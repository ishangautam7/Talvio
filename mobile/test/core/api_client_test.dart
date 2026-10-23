import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:zynveliq_mobile/src/core/api_client.dart';
import 'package:zynveliq_mobile/src/core/config.dart';

import '../helpers.dart';

http.Response jsonResponse(
  Object? body,
  int status, {
  Map<String, String>? headers,
}) => http.Response(
  jsonEncode(body),
  status,
  headers: {'content-type': 'application/json', ...?headers},
);

http.Response errorResponse(
  String code,
  int status, {
  Map<String, String>? headers,
}) => jsonResponse(
  {
    'error': {'code': code, 'message': 'English sentence'},
  },
  status,
  headers: headers,
);

Matcher apiError(String code) =>
    throwsA(isA<ApiException>().having((e) => e.code, 'code', code));

void main() {
  test('logIn posts credentials and device, and parses the session', () async {
    final session = await sessionStore(signedIn: false);
    late http.Request sent;
    final api = ApiClient(
      session,
      httpClient: MockClient((request) async {
        sent = request;
        return jsonResponse(authJson(), 200);
      }),
    );

    final auth = await api.logIn(
      email: 'a@b.com',
      password: 'secret-pw',
      device: thisDevice(),
    );

    expect(sent.method, 'POST');
    expect(sent.url.toString(), '$server/v1/auth/login');
    expect(sent.headers['Authorization'], isNull);
    expect(jsonDecode(sent.body), {
      'email': 'a@b.com',
      'password': 'secret-pw',
      'device': {
        'name': 'Android phone',
        'platform': 'android',
        'kind': 'mobile',
        'appVersion': appVersion,
      },
    });
    expect(auth.token, 'zv1_token');
    expect(auth.device.id, 'phone');
  });

  test('reads UTF-8 even without a charset', () async {
    final session = await sessionStore(signedIn: false);
    final api = ApiClient(
      session,
      httpClient: MockClient(
        (_) async => http.Response.bytes(
          utf8.encode(
            jsonEncode({...authJson(), 'user': userJson(name: 'ईशान')}),
          ),
          200,
        ),
      ),
    );
    final auth = await api.logIn(
      email: 'a@b.com',
      password: 'x',
      device: thisDevice(),
    );
    expect(auth.user.name, 'ईशान');
  });

  test('signUp leaves out a missing name', () async {
    final session = await sessionStore(signedIn: false);
    late http.Request sent;
    final api = ApiClient(
      session,
      httpClient: MockClient((request) async {
        sent = request;
        return jsonResponse(authJson(), 201);
      }),
    );
    await api.signUp(
      email: 'a@b.com',
      password: 'secret-pw',
      name: null,
      device: thisDevice(),
    );
    expect(jsonDecode(sent.body), isNot(contains('name')));
  });

  test('server error codes come through as ApiException', () async {
    final session = await sessionStore(signedIn: false);
    for (final (status, code) in [
      (400, 'invalid_email'),
      (400, 'weak_password'),
      (401, 'invalid_credentials'),
      (409, 'email_taken'),
      (503, 'google_unavailable'),
    ]) {
      final api = ApiClient(
        session,
        httpClient: MockClient((_) async => errorResponse(code, status)),
      );
      await expectLater(
        api.logIn(email: 'a@b.com', password: 'x', device: thisDevice()),
        apiError(code),
      );
    }
  });

  test('rate_limited carries Retry-After', () async {
    final session = await sessionStore(signedIn: false);
    final api = ApiClient(
      session,
      httpClient: MockClient(
        (_) async =>
            errorResponse('rate_limited', 429, headers: {'retry-after': '42'}),
      ),
    );
    await expectLater(
      api.logIn(email: 'a@b.com', password: 'x', device: thisDevice()),
      throwsA(
        isA<ApiException>().having(
          (e) => e.retryAfter,
          'retryAfter',
          const Duration(seconds: 42),
        ),
      ),
    );
  });

  test('unreachable server is network; garbage answers are internal', () async {
    final session = await sessionStore(signedIn: false);
    final offline = ApiClient(
      session,
      httpClient: MockClient((_) => throw http.ClientException('refused')),
    );
    await expectLater(offline.googleEnabled(), apiError('network'));

    final proxy = ApiClient(
      session,
      httpClient: MockClient(
        (_) async => http.Response('<html>Bad gateway</html>', 502),
      ),
    );
    await expectLater(proxy.googleEnabled(), apiError('internal'));

    final malformed = ApiClient(
      session,
      httpClient: MockClient((_) async => jsonResponse({'token': 1}, 200)),
    );
    await expectLater(
      malformed.logIn(email: 'a@b.com', password: 'x', device: thisDevice()),
      apiError('internal'),
    );
  });

  test('authenticated calls send the token to the session server', () async {
    final session = await sessionStore();
    late http.Request sent;
    final api = ApiClient(
      session,
      httpClient: MockClient((request) async {
        sent = request;
        return http.Response('', 204);
      }),
    );
    await api.removeDevice('pc/../x');
    expect(sent.method, 'DELETE');
    expect(sent.url.toString(), '$server/v1/devices/pc%2F..%2Fx');
    expect(sent.headers['Authorization'], 'Bearer zv1_token');
  });

  test('unauthorized on an authenticated call signs the phone out', () async {
    final session = await sessionStore();
    final api = ApiClient(
      session,
      httpClient: MockClient((_) async => errorResponse('unauthorized', 401)),
    );
    await expectLater(api.deleteAccount(), apiError('unauthorized'));
    expect(session.signedIn, isFalse);
  });

  test('googleEnabled and the Google start URL', () async {
    final session = await sessionStore(signedIn: false);
    final api = ApiClient(
      session,
      httpClient: MockClient(
        (_) async => jsonResponse({'protocol': 1, 'google': true}, 200),
      ),
    );
    expect(await api.googleEnabled(), isTrue);

    final start = api.googleStartUri(
      redirectUri: 'zynveliq://auth/callback',
      challenge: 'abc',
    );
    expect(
      start.toString().startsWith('$server/v1/auth/google/start?'),
      isTrue,
    );
    expect(start.queryParameters, {
      'redirect_uri': 'zynveliq://auth/callback',
      'code_challenge': 'abc',
      'code_challenge_method': 'S256',
    });
  });
}
