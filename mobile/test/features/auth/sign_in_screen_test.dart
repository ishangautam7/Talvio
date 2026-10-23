import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:zynveliq_mobile/src/core/api_client.dart';
import 'package:zynveliq_mobile/src/core/app_services.dart';
import 'package:zynveliq_mobile/src/core/relay_client.dart';
import 'package:zynveliq_mobile/src/core/server_finder.dart';
import 'package:zynveliq_mobile/src/core/session_store.dart';
import 'package:zynveliq_mobile/src/features/auth/sign_in_screen.dart';
import 'package:zynveliq_mobile/src/theme.dart';

import '../../helpers.dart';

typedef Handler = Future<http.Response> Function(http.Request request);

http.Response json(Object body, [int status = 200]) => http.Response(
  jsonEncode(body),
  status,
  headers: {'content-type': 'application/json'},
);

http.Response error(String code, int status) => json({
  'error': {'code': code, 'message': 'English sentence'},
}, status);

/// Serves /v1/config with [google], and everything else with [auth].
Handler server({bool google = false, Handler? auth}) => (request) async {
  if (request.url.path == '/v1/config') {
    return json({'protocol': 1, 'google': google});
  }
  return auth!(request);
};

/// The sign-in screen over [session], by default signed out on a Custom
/// server; [handler] answers the account server's API.
Future<SessionStore> pumpSignIn(
  WidgetTester tester,
  Handler handler, {
  SessionStore? session,
}) async {
  session ??= await sessionStore(signedIn: false);
  await tester.pumpWidget(
    AppServices(
      session: session,
      api: ApiClient(session, httpClient: MockClient(handler)),
      relay: RelayClient(session),
      prefs: await SharedPreferences.getInstance(),
      child: MaterialApp(
        theme: buildTheme(Brightness.light),
        home: const SignInScreen(),
      ),
    ),
  );
  await tester.pump();
  return session;
}

final email = find.widgetWithText(TextFormField, 'Email');
final password = find.widgetWithText(TextFormField, 'Password');
final signInButton = find.widgetWithText(FilledButton, 'Sign in');
final google = find.text('Continue with Google');

/// A release build in Automatic mode, signed out; [discovery] answers the
/// discovery address.
Future<SessionStore> automatic(Handler discovery, {String? pinned}) async {
  SharedPreferences.setMockInitialValues({});
  FlutterSecureStorage.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  final session = SessionStore(
    prefs,
    ServerFinder(prefs, httpClient: MockClient(discovery), release: true),
    pinned: pinned,
  );
  await session.load();
  return session;
}

/// What the account server answers the sign-in screen.
Handler config(List<String> hosts) => (request) async {
  hosts.add(request.url.authority);
  return json({'protocol': 1, 'google': false});
};

Future<void> fillIn(WidgetTester tester) async {
  await tester.enterText(email, ' A@b.com ');
  await tester.enterText(password, 'secret-pw');
}

void main() {
  testWidgets('hides Google when the server has no credentials', (
    tester,
  ) async {
    await pumpSignIn(tester, server());
    expect(google, findsNothing);
    expect(find.text('Custom · sync.test:8787'), findsOneWidget);
  });

  testWidgets('offers Google when the server does', (tester) async {
    await pumpSignIn(tester, server(google: true));
    expect(google, findsOneWidget);
  });

  testWidgets('asks for Google again when the server was not reachable', (
    tester,
  ) async {
    var reachable = false;
    await pumpSignIn(tester, (request) async {
      if (!reachable) throw http.ClientException('offline');
      if (request.url.path == '/v1/config') {
        return json({'protocol': 1, 'google': true});
      }
      return error('invalid_credentials', 401);
    });
    expect(google, findsNothing);

    // Back on its own after a while.
    reachable = true;
    await tester.pump(const Duration(seconds: 15));
    await tester.pump();
    expect(google, findsOneWidget);
  });

  testWidgets('a failed sign-in asks for Google again at once', (tester) async {
    var reachable = false;
    await pumpSignIn(tester, (request) async {
      if (!reachable) throw http.ClientException('offline');
      if (request.url.path == '/v1/config') {
        return json({'protocol': 1, 'google': true});
      }
      return error('invalid_credentials', 401);
    });
    reachable = true;
    await fillIn(tester);
    await tester.tap(signInButton);
    await tester.pump();
    await tester.pump();
    expect(find.textContaining('use Continue with Google'), findsOneWidget);
    expect(google, findsOneWidget);
  });

  testWidgets('Create account adds an optional name and its own button', (
    tester,
  ) async {
    await pumpSignIn(tester, server());
    expect(find.widgetWithText(TextFormField, 'Name (optional)'), findsNothing);

    await tester.tap(find.text('Create account'));
    await tester.pump();

    expect(
      find.widgetWithText(TextFormField, 'Name (optional)'),
      findsOneWidget,
    );
    expect(find.widgetWithText(FilledButton, 'Create account'), findsOneWidget);
    expect(find.text('At least 8 characters'), findsOneWidget);

    await tester.enterText(email, 'a@b.com');
    await tester.enterText(password, 'short');
    await tester.tap(find.widgetWithText(FilledButton, 'Create account'));
    await tester.pump();
    expect(find.text('Use at least 8 characters.'), findsOneWidget);
  });

  testWidgets('checks the form before calling the server', (tester) async {
    var calls = 0;
    await pumpSignIn(
      tester,
      server(
        auth: (_) async {
          calls++;
          return json(authJson());
        },
      ),
    );

    await tester.tap(signInButton);
    await tester.pump();
    expect(find.text('Enter your email.'), findsOneWidget);
    expect(find.text('Enter your password.'), findsOneWidget);

    await tester.enterText(email, 'not-an-email');
    await tester.tap(signInButton);
    await tester.pump();
    expect(find.text('Enter a valid email address.'), findsOneWidget);
    expect(calls, 0);
  });

  testWidgets('shows progress, then the server error, keeping the form', (
    tester,
  ) async {
    final reply = Completer<http.Response>();
    late http.Request sent;
    await pumpSignIn(
      tester,
      server(
        auth: (request) {
          sent = request;
          return reply.future;
        },
      ),
    );
    await fillIn(tester);
    await tester.tap(signInButton);
    await tester.pump();

    expect(
      find.descendant(
        of: find.byType(FilledButton),
        matching: find.byType(CircularProgressIndicator),
      ),
      findsOneWidget,
    );
    expect(tester.widget<TextFormField>(email).enabled, isFalse);
    expect(sent.url.path, '/v1/auth/login');
    // Trimmed here; the server lower-cases.
    expect(jsonDecode(sent.body), containsPair('email', 'A@b.com'));

    reply.complete(error('invalid_credentials', 401));
    await tester.pump();
    await tester.pump();

    expect(find.byType(CircularProgressIndicator), findsNothing);
    expect(
      find.textContaining("That email and password don't match"),
      findsOneWidget,
    );
    expect(tester.widget<TextFormField>(email).enabled, isTrue);
  });

  testWidgets('says when the server cannot be reached', (tester) async {
    await pumpSignIn(
      tester,
      server(auth: (_) => throw http.ClientException('refused')),
    );
    await fillIn(tester);
    await tester.tap(signInButton);
    await tester.pump();
    await tester.pump();
    expect(find.textContaining("Can't reach the server"), findsOneWidget);
  });

  testWidgets('a successful sign-in stores the session', (tester) async {
    final session = await pumpSignIn(
      tester,
      server(auth: (_) async => json(authJson())),
    );
    await fillIn(tester);
    await tester.tap(signInButton);
    await tester.pump();
    await tester.pump();

    expect(session.signedIn, isTrue);
    expect(session.session!.token, 'zv1_token');
    expect(session.session!.serverUrl, 'http://sync.test:8787');
  });

  testWidgets('the server address can be changed, and is validated', (
    tester,
  ) async {
    final hosts = <String>[];
    await pumpSignIn(tester, (request) async {
      hosts.add(request.url.host);
      return json({'protocol': 1, 'google': false});
    });

    await tester.tap(find.text('Change'));
    await tester.pumpAndSettle();
    final field = find.widgetWithText(TextFormField, 'Server address');
    await tester.enterText(field, 'ftp://nope');
    await tester.tap(find.text('Save'));
    await tester.pump();
    expect(
      find.text('Enter an address like https://sync.example.com'),
      findsOneWidget,
    );

    await tester.enterText(field, 'new.example.com/');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    expect(find.text('Custom · new.example.com'), findsOneWidget);
    expect(hosts, ['sync.test', 'new.example.com']);
  });

  testWidgets('Automatic: looking, then not found with Try again, then found', (
    tester,
  ) async {
    tester.view
      ..physicalSize = const Size(320, 568)
      ..devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    var discovery = Completer<http.Response>();
    final hosts = <String>[];
    final session = await automatic((_) => discovery.future);
    await pumpSignIn(tester, config(hosts), session: session);

    expect(find.text('Looking for the server…'), findsOneWidget);
    expect(find.text('Try again'), findsNothing);
    expect(find.text('Change'), findsOneWidget);

    discovery.complete(http.Response('', 503));
    await tester.pump();
    await tester.pump();
    expect(find.text("Can't find the server"), findsOneWidget);
    expect(tester.takeException(), isNull, reason: 'fits a small phone');
    expect(hosts, isEmpty, reason: 'no server to ask yet');

    discovery = Completer()..complete(json({'domain': 'sync.example.com'}));
    await tester.ensureVisible(find.text('Try again'));
    await tester.tap(find.text('Try again'));
    await tester.pump();
    await tester.pump();
    expect(find.text('Automatic · sync.example.com'), findsOneWidget);
    expect(find.text('Try again'), findsNothing);
    expect(hosts, ['sync.example.com']);
  });

  testWidgets('a Custom server, then back to Automatic', (tester) async {
    final hosts = <String>[];
    final session = await automatic(
      (_) async => json({'domain': 'sync.example.com'}),
    );
    await pumpSignIn(tester, config(hosts), session: session);
    await tester.pump();
    expect(find.text('Automatic · sync.example.com'), findsOneWidget);

    await tester.tap(find.text('Change'));
    await tester.pumpAndSettle();
    expect(find.text('Use automatic'), findsNothing);
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Server address'),
      'custom.example.com',
    );
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();
    expect(find.text('Custom · custom.example.com'), findsOneWidget);
    expect(session.serverChoice, ServerChoice.custom);

    await tester.tap(find.text('Change'));
    await tester.pumpAndSettle();
    expect(find.text('https://custom.example.com'), findsOneWidget);
    await tester.tap(find.text('Use automatic'));
    await tester.pumpAndSettle();
    expect(find.text('Automatic · sync.example.com'), findsOneWidget);
    expect(hosts, [
      'sync.example.com',
      'custom.example.com',
      'sync.example.com',
    ]);
  });

  testWidgets('a server built in cannot be changed', (tester) async {
    var lookups = 0;
    final session = await automatic((_) async {
      lookups++;
      return json({'domain': 'sync.example.com'});
    }, pinned: 'https://pinned.example.com');
    await pumpSignIn(tester, config([]), session: session);

    expect(find.text('Server: pinned.example.com'), findsOneWidget);
    expect(find.text('Change'), findsNothing);
    expect(lookups, 0);
  });
}
