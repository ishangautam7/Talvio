import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/app.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/relay_client.dart';

import 'helpers.dart';

/// The whole signed-in app on a small phone (iPhone SE size, 320×568), with
/// [devices] on the account. With [recordEvents], the recorder records and
/// its state events go to the sink.
Future<(FakeServer, RelayClient)> pumpApp(
  WidgetTester tester,
  List<Map<String, Object?>> devices, {
  void Function(MockStreamHandlerEventSink events)? recordEvents,
}) async {
  tester.view
    ..physicalSize = const Size(320, 568)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final messenger = tester.binding.defaultBinaryMessenger;
  // No plugins under test: no temporary directory, a recorder that does
  // nothing (and has no microphone permission unless [recordEvents] is set).
  messenger.setMockMethodCallHandler(
    const MethodChannel('plugins.flutter.io/path_provider'),
    (_) async => '/nonexistent-talvio-test',
  );
  messenger.setMockMethodCallHandler(
    const MethodChannel('com.llfbandit.record/messages'),
    (call) async {
      if (recordEvents == null) return null;
      if (call.method == 'create') {
        final id = (call.arguments as Map)['recorderId'];
        messenger.setMockStreamHandler(
          EventChannel('com.llfbandit.record/events/$id'),
          MockStreamHandler.inline(
            onListen: (_, sink) {
              recordEvents(sink);
            },
          ),
        );
      }
      return call.method == 'hasPermission' ? true : null;
    },
  );

  final session = await sessionStore();
  final server = FakeServer();
  final relay = RelayClient(session, connectChannel: server.connect);
  await tester.pumpWidget(
    TalvioApp(
      session: session,
      api: ApiClient(
        session,
        httpClient: MockClient((_) async => http.Response('', 204)),
      ),
      relay: relay,
      prefs: await SharedPreferences.getInstance(),
    ),
  );
  await tester.pump();
  server
    ..send({'type': 'ready', 'deviceId': 'phone', 'user': userJson()})
    ..send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        ...devices,
      ],
    });
  await tester.pump();
  await tester.pump();
  return (server, relay);
}

/// Types [text], sends it, and returns the request's id.
Future<String> sendText(
  WidgetTester tester,
  FakeServer server, [
  String text = 'Hello',
]) async {
  await tester.enterText(find.byType(TextField), text);
  await tester.pump();
  await tester.tap(find.byTooltip('Send'));
  await tester.pump();
  final body = server.relays.last['body']! as Map<String, Object?>;
  return body['requestId']! as String;
}

/// The computer reports [stage] for request [id].
Future<void> progress(
  WidgetTester tester,
  FakeServer server,
  String id,
  String stage,
) async {
  server.relay('pc', {'kind': 'progress', 'requestId': id, 'stage': stage});
  await tester.pump();
  await tester.pump();
}

/// The account's computers are now [computers].
Future<void> computersAre(
  WidgetTester tester,
  FakeServer server,
  List<Map<String, Object?>> computers,
) async {
  server.send({
    'type': 'devices',
    'devices': [
      deviceJson(id: 'phone', kind: 'mobile', current: true),
      ...computers,
    ],
  });
  await tester.pump();
  await tester.pump();
}

/// The phone locks, or unlocks, as the platform reports it.
void lock(WidgetTester tester, {bool locked = true}) {
  final states = locked
      ? [AppLifecycleState.inactive, AppLifecycleState.hidden]
      : [AppLifecycleState.hidden, AppLifecycleState.inactive];
  for (final state in [
    ...states,
    locked ? AppLifecycleState.paused : AppLifecycleState.resumed,
  ]) {
    tester.binding.handleAppLifecycleStateChanged(state);
  }
}

const timedOut = "ISHAN-PC didn't answer in time. Try again.";

/// Unmounts the app and stops the socket's timers.
Future<void> finish(WidgetTester tester, RelayClient relay) async {
  await tester.pumpWidget(const SizedBox());
  relay.dispose();
}

void main() {
  testWidgets('no computer yet: says how to add one', (tester) async {
    final (_, relay) = await pumpApp(tester, []);
    expect(find.text('Add your computer'), findsOneWidget);
    expect(find.textContaining('sign in with a@b.com'), findsOneWidget);
    expect(find.byType(NavigationBar), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('computer offline: says so and keeps the mic off', (
    tester,
  ) async {
    final (_, relay) = await pumpApp(tester, [deviceJson(online: false)]);
    expect(find.textContaining('ISHAN-PC is offline'), findsOneWidget);
    final mic = tester.widget<FilledButton>(
      find.ancestor(
        of: find.byIcon(Icons.mic),
        matching: find.byType(FilledButton),
      ),
    );
    expect(mic.onPressed, isNull);
    await finish(tester, relay);
  });

  testWidgets('screen readers can press the mic', (tester) async {
    final handle = tester.ensureSemantics();
    final (_, relay) = await pumpApp(tester, [deviceJson()]);
    expect(
      tester.getSemantics(find.bySemanticsLabel('Start dictating')),
      isSemantics(isButton: true, isEnabled: true, hasTapAction: true),
    );
    await finish(tester, relay);
    handle.dispose();
  });

  testWidgets('landscape and 200% text fit, recording included', (
    tester,
  ) async {
    for (final (size, scale) in [
      (const Size(640, 360), 1.0),
      (const Size(360, 640), 2.0),
    ]) {
      final (_, relay) = await pumpApp(tester, [
        deviceJson(),
      ], recordEvents: (_) {});
      tester.view.physicalSize = size;
      tester.platformDispatcher.textScaleFactorTestValue = scale;
      await tester.pump();
      expect(tester.takeException(), isNull, reason: '$size idle');
      final mic = find.widgetWithIcon(FilledButton, Icons.mic);
      await tester.ensureVisible(mic);
      await tester.tap(mic);
      await tester.pump();
      await tester.pump();
      expect(find.text('Discard'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: '$size recording');
      await tester.ensureVisible(find.text('Discard'));
      await tester.tap(find.text('Discard'));
      await finish(tester, relay);
      tester.platformDispatcher.clearTextScaleFactorTestValue();
    }
  });

  testWidgets('a call pausing the recorder shows as paused until it resumes', (
    tester,
  ) async {
    MockStreamHandlerEventSink? recorder;
    final (_, relay) = await pumpApp(tester, [
      deviceJson(),
    ], recordEvents: (events) => recorder = events);
    await tester.tap(find.widgetWithIcon(FilledButton, Icons.mic));
    await tester.pump();
    await tester.pump();
    expect(find.textContaining('0:00 · Tap to send'), findsOneWidget);

    recorder!.success(0); // RecordState.pause
    await tester.pump();
    expect(find.text('Paused · Tap to send'), findsOneWidget);
    recorder!.success(1); // RecordState.record
    await tester.pump();
    expect(find.textContaining('0:00 · Tap to send'), findsOneWidget);

    await tester.tap(find.text('Discard'));
    await finish(tester, relay);
  });

  testWidgets('a long computer name fits a small phone', (tester) async {
    const long = 'A-VERY-LONG-WORKSTATION-NAME-IN-THE-OFFICE-2';
    final (_, relay) = await pumpApp(tester, [
      deviceJson(name: long),
      deviceJson(id: 'b', name: 'LAPTOP'),
    ]);
    expect(tester.takeException(), isNull, reason: 'picker');
    await tester.tap(find.text(long));
    await tester.pump();
    expect(tester.takeException(), isNull);
    expect(find.text('Tap to speak'), findsOneWidget);
    expect(find.byIcon(Icons.arrow_drop_down), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('several computers: the user picks one, and it is remembered', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [
      deviceJson(),
      deviceJson(id: 'b', name: 'LAPTOP', online: false),
    ]);
    expect(find.text('Choose a computer'), findsOneWidget);
    expect(find.widgetWithIcon(FilledButton, Icons.mic), findsNothing);

    await tester.tap(find.text('LAPTOP'));
    await tester.pump();
    expect(find.text('Choose a computer'), findsNothing);
    expect(find.textContaining('LAPTOP is offline'), findsOneWidget);
    final prefs = await SharedPreferences.getInstance();
    expect(prefs.getString('account.dictate.computer'), 'b');

    // A computer joining later does not take over the choice.
    server.send({
      'type': 'devices',
      'devices': [
        deviceJson(id: 'phone', kind: 'mobile', current: true),
        deviceJson(),
        deviceJson(id: 'b', name: 'LAPTOP', online: false),
        deviceJson(id: 'new', name: 'NEW-PC'),
      ],
    });
    await tester.pump();
    await tester.pump();
    expect(find.textContaining('LAPTOP is offline'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('a computer that joined later is never chosen on its own', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    expect(find.text('Tap to speak'), findsOneWidget, reason: 'only one');

    await computersAre(tester, server, [
      deviceJson(),
      deviceJson(id: 'new', name: 'NEW-PC'),
    ]);
    expect(find.text('ISHAN-PC'), findsOneWidget, reason: 'still the one');
    expect(find.text('Tap to speak'), findsOneWidget);
    expect(find.byIcon(Icons.arrow_drop_down), findsOneWidget);

    // Once ISHAN-PC leaves the account, NEW-PC is its only computer, and
    // still not chosen for the user.
    await computersAre(tester, server, [deviceJson(id: 'new', name: 'NEW-PC')]);
    expect(find.text('Choose a computer'), findsOneWidget);
    expect(find.text('Tap to speak'), findsNothing);
    await tester.tap(find.text('NEW-PC'));
    await tester.pump();
    expect(find.text('Tap to speak'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('a request and a recording keep their computer to the end', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [
      deviceJson(),
    ], recordEvents: (_) {});

    // ISHAN-PC leaves the account while it asks to allow this phone.
    final id = await sendText(tester, server);
    await progress(tester, server, id, 'approval');
    await computersAre(tester, server, [deviceJson(id: 'new', name: 'NEW-PC')]);
    expect(find.text('Allow this phone on ISHAN-PC'), findsOneWidget);
    // Under the keyboard here; the button itself is what matters.
    tester
        .widget<TextButton>(find.widgetWithText(TextButton, 'Cancel'))
        .onPressed!();
    await tester.pump();
    expect(server.relays.last['body'], {'kind': 'cancel', 'requestId': id});
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': false,
      'error': 'cancelled',
    });
    await tester.pump();
    await tester.pump();
    expect(find.text('Choose a computer'), findsOneWidget);

    // NEW-PC leaves while the user records for it.
    await tester.tap(find.text('NEW-PC'));
    await tester.pump();
    await tester.tap(find.widgetWithIcon(FilledButton, Icons.mic));
    await tester.pump();
    await tester.pump();
    await computersAre(tester, server, [
      deviceJson(id: 'other', name: 'OTHER-PC'),
    ]);
    expect(find.text('Choose a computer'), findsNothing);
    expect(find.text('NEW-PC'), findsOneWidget);
    await tester.tap(find.text('Discard'));
    await tester.pump();
    expect(find.text('Choose a computer'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('typed text is pasted on the computer', (tester) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    await tester.enterText(find.byType(TextField), '  Hello there  ');
    await tester.pump();
    await tester.tap(find.byTooltip('Send'));
    await tester.pump();
    await tester.pump();

    final sent = server.relays.single;
    expect(sent['to'], 'pc');
    final body = sent['body']! as Map<String, Object?>;
    expect(body['kind'], 'text');
    expect(body['text'], 'Hello there');
    expect(find.text('Pasting on ISHAN-PC…'), findsOneWidget);

    server.relay('pc', {
      'kind': 'result',
      'requestId': body['requestId'],
      'ok': true,
      'pasted': true,
    });
    await tester.pump();
    await tester.pump();

    expect(find.text('Sent to ISHAN-PC and pasted.'), findsOneWidget);
    expect(find.text('Hello there'), findsNothing, reason: 'field cleared');
    await finish(tester, relay);
  });

  testWidgets('no answer within 60 s is a timeout, and cancels', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    await tester.enterText(find.byType(TextField), 'Hello');
    await tester.pump();
    await tester.tap(find.byTooltip('Send'));
    await tester.pump();
    final requestId =
        (server.relays.single['body']! as Map<String, Object?>)['requestId'];

    await tester.pump(const Duration(seconds: 61));

    expect(
      find.text("ISHAN-PC didn't answer in time. Try again."),
      findsOneWidget,
    );
    expect(server.relays.last['body'], {
      'kind': 'cancel',
      'requestId': requestId,
    });
    expect(find.text('Hello'), findsOneWidget, reason: 'text kept to resend');
    await finish(tester, relay);
  });

  testWidgets('while the computer asks to allow this phone, waits 3 minutes', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    final id = await sendText(tester, server);
    await progress(tester, server, id, 'approval');

    expect(find.text('Allow this phone on ISHAN-PC'), findsOneWidget);
    expect(
      find.text("Look at the computer's screen. Talvio is asking there."),
      findsOneWidget,
    );
    final cancel = tester.widget<TextButton>(
      find.widgetWithText(TextButton, 'Cancel'),
    );
    expect(cancel.onPressed, isNotNull);

    await tester.pump(const Duration(minutes: 2, seconds: 59));
    expect(find.text(timedOut), findsNothing);
    expect(find.text('Allow this phone on ISHAN-PC'), findsOneWidget);

    await tester.pump(const Duration(seconds: 2));
    await tester.pump();
    expect(find.text(timedOut), findsOneWidget);
    expect(server.relays.last['body'], {'kind': 'cancel', 'requestId': id});
    await finish(tester, relay);
  });

  testWidgets('every progress restarts the wait, 60 s again after approval', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    final id = await sendText(tester, server);

    await tester.pump(const Duration(seconds: 50));
    await progress(tester, server, id, 'pasting');
    await tester.pump(const Duration(seconds: 50));
    expect(find.text(timedOut), findsNothing);

    await progress(tester, server, id, 'approval');
    await tester.pump(const Duration(minutes: 2, seconds: 50));
    await progress(tester, server, id, 'pasting');
    expect(find.text('Pasting on ISHAN-PC…'), findsOneWidget);
    await tester.pump(const Duration(seconds: 59));
    expect(find.text(timedOut), findsNothing);

    await tester.pump(const Duration(seconds: 2));
    await tester.pump();
    expect(find.text(timedOut), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('a locked phone keeps the socket until the result arrives', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    final id = await sendText(tester, server);
    await progress(tester, server, id, 'approval');

    // The user turns to the computer to allow this phone; the phone locks.
    lock(tester);
    await tester.pump();
    expect(relay.status.value, RelayStatus.connected);
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': true,
      'pasted': true,
    });
    await tester.pump();
    expect(relay.status.value, RelayStatus.disconnected, reason: 'done');

    lock(tester, locked: false);
    await tester.pump();
    await tester.pump();
    expect(find.text('Sent to ISHAN-PC and pasted.'), findsOneWidget);
    expect(find.text('Hello'), findsNothing, reason: 'nothing to resend');
    await finish(tester, relay);
  });

  testWidgets('a socket lost mid-request says to check the computer', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    final id = await sendText(tester, server);
    await progress(tester, server, id, 'approval');

    // Suspended for longer than the server keeps a silent socket.
    lock(tester);
    await tester.pump();
    await server.socket.close(1000, 'idle');
    await tester.pump();
    lock(tester, locked: false);
    await tester.pump();
    await tester.pump();

    expect(find.text('Allow this phone on ISHAN-PC'), findsNothing);
    expect(
      find.text(
        'Lost the connection before ISHAN-PC answered, so it may have '
        'finished anyway. Check ISHAN-PC before you try again.',
      ),
      findsOneWidget,
    );
    await finish(tester, relay);
  });

  testWidgets('not_approved says where to allow this phone', (tester) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    final id = await sendText(tester, server);
    await progress(tester, server, id, 'approval');
    server.relay('pc', {
      'kind': 'result',
      'requestId': id,
      'ok': false,
      'error': 'not_approved',
    });
    await tester.pump();
    await tester.pump();

    expect(
      find.text(
        "This phone isn't allowed on ISHAN-PC. Allow it in Talvio's "
        'Settings on that computer, under Account, then try again.',
      ),
      findsOneWidget,
    );
    expect(find.text('Hello'), findsOneWidget, reason: 'text kept to resend');
    await finish(tester, relay);
  });

  testWidgets('relay_failed says the computer is offline', (tester) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    await tester.enterText(find.byType(TextField), 'Hello');
    await tester.pump();
    await tester.tap(find.byTooltip('Send'));
    await tester.pump();
    final id = server.frames.last['id'];
    server.send({
      'type': 'relay_failed',
      'id': id,
      'to': 'pc',
      'reason': 'offline',
    });
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('ISHAN-PC is offline'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('a computer reconnecting keeps the outcome and typed text', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    await tester.enterText(find.byType(TextField), 'Hello');
    await tester.pump();
    await tester.tap(find.byTooltip('Send'));
    await tester.pump();
    server.send({
      'type': 'relay_failed',
      'id': server.frames.last['id'],
      'to': 'pc',
      'reason': 'offline',
    });
    await tester.pump();

    // The server lists a reconnecting computer before its `hello` arrives.
    for (final pc in [
      deviceJson(online: false),
      deviceJson(capabilities: []),
      deviceJson(),
    ]) {
      server.send({
        'type': 'devices',
        'devices': [deviceJson(id: 'phone', kind: 'mobile', current: true), pc],
      });
      await tester.pump();
      await tester.pump();
      expect(find.byType(NavigationBar), findsOneWidget);
      expect(
        find.textContaining('on it and try again', skipOffstage: false),
        findsOneWidget,
      );
      expect(find.text('Hello'), findsOneWidget);
      expect(find.textContaining('Update Talvio'), findsNothing);
    }
    await finish(tester, relay);
  });

  testWidgets('an online computer without dictation hides the tab', (
    tester,
  ) async {
    final (_, relay) = await pumpApp(tester, [
      deviceJson(capabilities: ['text']),
    ]);
    expect(find.byType(NavigationBar), findsNothing);
    expect(find.text('Sign out'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('account lists devices and confirms a removal', (tester) async {
    final (_, relay) = await pumpApp(tester, [deviceJson(online: false)]);
    await tester.tap(find.text('Account'));
    await tester.pumpAndSettle();

    expect(find.text('This phone'), findsOneWidget);
    expect(find.text('Ishan'), findsOneWidget);
    expect(find.textContaining('Last seen'), findsOneWidget);
    expect(find.text('Custom · sync.test:8787'), findsOneWidget);

    await tester.tap(find.byTooltip('Remove ISHAN-PC'));
    await tester.pumpAndSettle();
    expect(find.text('Remove ISHAN-PC?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.text('Remove ISHAN-PC?'), findsNothing);
    await finish(tester, relay);
  });
}
