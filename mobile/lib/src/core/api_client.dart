import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import 'models.dart';
import 'session_store.dart';

/// A failed call. [code] is the server's error code from PROTOCOL.md, or
/// `network` when the server could not be reached. Callers show text per code
/// and a generic message for any code they do not know.
class ApiException implements Exception {
  const ApiException(this.code, {this.retryAfter});

  final String code;

  /// From `Retry-After` on `rate_limited`.
  final Duration? retryAfter;

  @override
  String toString() => 'ApiException($code)';
}

/// The account server's HTTP API. Unauthenticated calls go to the server
/// shown on the sign-in screen; authenticated ones to the session's own
/// server. A revoked token (`unauthorized`) signs this phone out, and a server
/// that does not answer at all is looked up again (discovery).
class ApiClient {
  ApiClient(this._session, {http.Client? httpClient})
    : _http = httpClient ?? http.Client();

  static const _timeout = Duration(seconds: 20);

  final SessionStore _session;
  final http.Client _http;

  /// Whether the server offers Google sign-in.
  Future<bool> googleEnabled() async {
    final json = await _call('GET', '/v1/config');
    return json?['google'] == true;
  }

  Future<AuthResponse> signUp({
    required String email,
    required String password,
    required String? name,
    required DeviceInfo device,
  }) => _auth(
    _call(
      'POST',
      '/v1/auth/signup',
      body: {
        'email': email,
        'password': password,
        'name': ?name,
        'device': device.toJson(),
      },
    ),
  );

  Future<AuthResponse> logIn({
    required String email,
    required String password,
    required DeviceInfo device,
  }) => _auth(
    _call(
      'POST',
      '/v1/auth/login',
      body: {'email': email, 'password': password, 'device': device.toJson()},
    ),
  );

  /// Where the Google sign-in starts, in a browser.
  Uri googleStartUri({required String redirectUri, required String challenge}) {
    final server = _session.serverUrl;
    if (server == null) throw const ApiException('network');
    return Uri.parse('$server/v1/auth/google/start').replace(
      queryParameters: {
        'redirect_uri': redirectUri,
        'code_challenge': challenge,
        'code_challenge_method': 'S256',
      },
    );
  }

  Future<AuthResponse> exchange({
    required String code,
    required String codeVerifier,
    required DeviceInfo device,
  }) => _auth(
    _call(
      'POST',
      '/v1/auth/exchange',
      body: {
        'code': code,
        'codeVerifier': codeVerifier,
        'device': device.toJson(),
      },
    ),
  );

  Future<void> logOut() => _call('POST', '/v1/auth/logout', auth: true);

  Future<void> deleteAccount() => _call('DELETE', '/v1/me', auth: true);

  Future<void> removeDevice(String id) =>
      _call('DELETE', '/v1/devices/${Uri.encodeComponent(id)}', auth: true);

  static Future<AuthResponse> _auth(Future<Map<String, Object?>?> call) async {
    final json = await call;
    try {
      return AuthResponse.fromJson(json);
    } on FormatException {
      throw const ApiException('internal');
    }
  }

  Future<Map<String, Object?>?> _call(
    String method,
    String path, {
    Map<String, Object?>? body,
    bool auth = false,
  }) async {
    final session = _session.session;
    if (auth && session == null) throw const ApiException('unauthorized');
    final base = auth ? session!.serverUrl : _session.serverUrl;
    if (base == null) {
      // A release build that has not found its server yet.
      _session.finder.unreachable();
      throw const ApiException('network');
    }
    final request = http.Request(method, Uri.parse('$base$path'))
      ..headers['Accept'] = 'application/json';
    if (auth) request.headers['Authorization'] = 'Bearer ${session!.token}';
    if (body != null) {
      request.headers['Content-Type'] = 'application/json';
      request.body = jsonEncode(body);
    }

    final http.Response response;
    try {
      response = await http.Response.fromStream(
        await _http.send(request).timeout(_timeout),
      ).timeout(_timeout);
    } on Exception {
      // Offline, DNS, refused, TLS, cleartext blocked, or too slow: to the
      // user these are all "can't reach the server". It may have moved.
      _session.finder.unreachable();
      throw const ApiException('network');
    }

    // All JSON is UTF-8 (PROTOCOL.md), whatever the headers say.
    final text = utf8.decode(response.bodyBytes, allowMalformed: true);
    final json = _decode(text);
    if (response.statusCode >= 200 && response.statusCode < 300) {
      if (text.isNotEmpty && json == null) {
        throw const ApiException('internal');
      }
      return json;
    }
    final code = switch (json) {
      {'error': {'code': String code}} => code,
      _ => 'internal',
    };
    if (auth && code == 'unauthorized') await _session.clear();
    final seconds = int.tryParse(response.headers['retry-after'] ?? '');
    throw ApiException(
      code,
      retryAfter: seconds == null ? null : Duration(seconds: seconds),
    );
  }

  static Map<String, Object?>? _decode(String body) {
    if (body.isEmpty) return null;
    try {
      final json = jsonDecode(body);
      return json is Map<String, Object?> ? json : null;
    } on FormatException {
      return null;
    }
  }
}
