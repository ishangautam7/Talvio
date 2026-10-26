import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/app_services.dart';
import 'package:talvio_mobile/src/core/relay_client.dart';
import 'package:talvio_mobile/src/core/session_store.dart';
import 'package:talvio_mobile/src/features/account/feedback.dart';
import 'package:talvio_mobile/src/features/account/feedback_screen.dart';
import 'package:talvio_mobile/src/features/account/strings.dart';
import 'package:talvio_mobile/src/theme.dart';

import '../../helpers.dart';

const line = 'Talvio phone 1.0.0 · Android 14';

http.Response created() => http.Response(
  jsonEncode({'id': 'f-1'}),
  201,
  headers: {'content-type': 'application/json'},
);

Future<SessionStore> pumpFeedback(
  WidgetTester tester,
  Future<http.Response> Function(http.Request) handler, {
  bool signedIn = true,
}) async {
  final session = await sessionStore(signedIn: signedIn);
  await tester.pumpWidget(
    AppServices(
      session: session,
      api: ApiClient(session, httpClient: MockClient(handler)),
      relay: RelayClient(session),
      prefs: await SharedPreferences.getInstance(),
      child: MaterialApp(
        theme: buildTheme(Brightness.light),
        home: const FeedbackScreen(systemLine: line),
      ),
    ),
  );
  await tester.pump();
  return session;
}

final message = find.widgetWithText(TextField, FeedbackStrings.message);
final email = find.widgetWithText(TextField, FeedbackStrings.email);
final send = find.text(FeedbackStrings.send);

void main() {
  group('FeedbackRules', () {
    test('a message needs a few characters, at most 5,000', () {
      expect(FeedbackRules.messageOk('  ok '), isFalse);
      expect(FeedbackRules.messageOk('the pill froze'), isTrue);
      expect(FeedbackRules.messageOk('x' * 5001), isFalse);
      // Code points, as the server counts them.
      expect(FeedbackRules.messageOk('🎙' * 5000), isTrue);
    });

    test('an email is optional but must look like one', () {
      expect(FeedbackRules.emailOk('  '), isTrue);
      expect(FeedbackRules.emailOk('me@example.com'), isTrue);
      for (final bad in ['a@b', 'a@@b.co', '@b.co', 'a b@c.co', 'a@b..co']) {
        expect(FeedbackRules.emailOk(bad), isFalse, reason: bad);
      }
    });

    test('the system line names the app and the phone', () {
      expect(
        FeedbackRules.systemLine(
          appVersion: '1.0.0',
          os: 'android',
          osVersion: '14',
        ),
        'Talvio phone 1.0.0 · Android 14',
      );
      expect(
        FeedbackRules.systemLine(appVersion: '1.0.0', os: 'ios', osVersion: ''),
        'Talvio phone 1.0.0 · iOS',
      );
      final long = FeedbackRules.systemLine(
        appVersion: '1.0.0',
        os: 'android',
        osVersion: 'x' * 400,
      );
      expect(long.runes.length, 300);
    });

    test('errors read as sentences', () {
      expect(
        FeedbackRules.errorText(const ApiException('invalid_email')),
        FeedbackStrings.emailInvalid,
      );
      expect(
        FeedbackRules.errorText(const ApiException('network')),
        FeedbackStrings.unreachable,
      );
      expect(
        FeedbackRules.errorText(const ApiException('rate_limited')),
        contains('Too many attempts'),
      );
    });
  });

  testWidgets('sends what the form shows, signed with the session', (
    tester,
  ) async {
    late http.Request sent;
    await pumpFeedback(tester, (request) async {
      sent = request;
      return created();
    });
    expect(find.text(FeedbackStrings.sentAs(line)), findsOneWidget);

    await tester.enterText(message, '  The keyboard tab lags.  ');
    await tester.enterText(email, 'me@example.com');
    await tester.pump();
    await tester.tap(send);
    await tester.pumpAndSettle();

    expect(sent.method, 'POST');
    expect(sent.url.toString(), '$server/v1/feedback');
    expect(sent.headers['Authorization'], 'Bearer zv1_token');
    expect(jsonDecode(sent.body), {
      'message': 'The keyboard tab lags.',
      'email': 'me@example.com',
      'system': line,
    });
    expect(find.text(FeedbackStrings.sentTitle), findsOneWidget);
    expect(
      find.text(FeedbackStrings.sentWithEmail('me@example.com')),
      findsOneWidget,
    );
    final prefs = await SharedPreferences.getInstance();
    expect(prefs.getString(feedbackDraftKey), isNull);
    expect(prefs.getString(feedbackEmailKey), 'me@example.com');
  });

  testWidgets('leaves out what was left out, and needs no session', (
    tester,
  ) async {
    late http.Request sent;
    await pumpFeedback(tester, (request) async {
      sent = request;
      return created();
    }, signedIn: false);

    await tester.enterText(message, 'An idea for the phone app');
    await tester.tap(find.text(FeedbackStrings.includeSystem));
    await tester.pump();
    await tester.tap(send);
    await tester.pumpAndSettle();

    expect(sent.headers['Authorization'], isNull);
    expect(jsonDecode(sent.body), {'message': 'An idea for the phone app'});
    expect(find.text(FeedbackStrings.sentBody), findsOneWidget);
  });

  testWidgets('Send waits for a message and a sensible email', (tester) async {
    var calls = 0;
    await pumpFeedback(tester, (_) async {
      calls++;
      return created();
    });
    await tester.tap(send);
    await tester.enterText(message, 'ok');
    await tester.pump();
    await tester.tap(send);
    await tester.enterText(message, 'A real report');
    await tester.enterText(email, 'nope');
    await tester.pump();
    expect(find.text(FeedbackStrings.emailInvalid), findsOneWidget);
    await tester.tap(send);
    await tester.pump();
    expect(calls, 0);
  });

  testWidgets('a failed send keeps the draft and says why', (tester) async {
    await pumpFeedback(
      tester,
      (_) async => http.Response(
        jsonEncode({
          'error': {'code': 'rate_limited', 'message': 'English sentence'},
        }),
        429,
        headers: {'content-type': 'application/json', 'retry-after': '120'},
      ),
    );
    await tester.enterText(message, 'Please keep this');
    await tester.pump();
    await tester.tap(send);
    await tester.pumpAndSettle();

    expect(find.textContaining('Try again in 2 minutes'), findsOneWidget);
    expect(find.text(FeedbackStrings.sentTitle), findsNothing);
    final prefs = await SharedPreferences.getInstance();
    expect(prefs.getString(feedbackDraftKey), 'Please keep this');
  });

  testWidgets('a saved draft comes back', (tester) async {
    SharedPreferences.setMockInitialValues({});
    final session = await sessionStore();
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(feedbackDraftKey, 'Half-written report');
    await tester.pumpWidget(
      AppServices(
        session: session,
        api: ApiClient(session, httpClient: MockClient((_) async => created())),
        relay: RelayClient(session),
        prefs: prefs,
        child: MaterialApp(
          theme: buildTheme(Brightness.light),
          home: const FeedbackScreen(systemLine: line),
        ),
      ),
    );
    await tester.pump();
    expect(find.text('Half-written report'), findsOneWidget);
  });
}
