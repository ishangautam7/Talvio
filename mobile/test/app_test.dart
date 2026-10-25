import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/app.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/relay_client.dart';
import 'package:talvio_mobile/src/design/components.dart';

import 'helpers.dart';

/// Shows [page] through the navigation bar (or the rail, on a wide window).
Future<void> goTo(WidgetTester tester, String page) async {
  final bar = find.byType(NavigationBar);
  await tester.tap(
    find.descendant(
      of: bar.evaluate().isNotEmpty ? bar : find.byType(NavigationRail),
      matching: find.text(page),
    ),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 500));
}

/// The Dictate page's big mic, and its stop.
final mic = find.byIcon(Icons.mic_rounded);
final stop = find.byIcon(Icons.stop_rounded);

/// Whether the button showing [icon] can be pressed.
bool enabled(WidgetTester tester, Finder icon) =>
    tester
        .widget<InkWell>(
          find.ancestor(of: icon, matching: find.byType(InkWell)).first,
        )
        .onTap !=
    null;

/// The whole signed-in app on a small phone (iPhone SE size, 320×568), with
/// [devices] on the account, showing [page] (home when null). With
/// [recordEvents], the recorder records and its state events go to the sink;
/// with [recording] too, stopping it answers that file.
Future<(FakeServer, RelayClient)> pumpApp(
  WidgetTester tester,
  List<Map<String, Object?>> devices, {
  void Function(MockStreamHandlerEventSink events)? recordEvents,
  String? recording,
  String? page = 'Dictate',
}) async {
  ambientMotion = false;
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
      return switch (call.method) {
        'hasPermission' => true,
        'stop' => recording,
        _ => null,
      };
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
  final bar = find.byType(NavigationBar);
  if (page != null &&
      find
          .descendant(of: bar, matching: find.text(page))
          .evaluate()
          .isNotEmpty) {
    await goTo(tester, page);
  }
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

/// The Dictate page, ready for a computer: its mic is there.
final hint = mic;

/// The labels of the navigation bar, in order.
List<String?> tabs(WidgetTester tester) => [
  for (final d in tester.widgetList<NavigationDestination>(
    find.byType(NavigationDestination),
  ))
    d.label,
];

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
    expect(find.byTooltip('Account'), findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('computer offline: says so and keeps the mic off', (
    tester,
  ) async {
    final (_, relay) = await pumpApp(tester, [deviceJson(online: false)]);
    await goTo(tester, 'Dictate');
    expect(find.textContaining('ISHAN-PC is offline'), findsOneWidget);
    expect(find.text('Offline'), findsOneWidget, reason: 'the top bar');
    expect(enabled(tester, mic), isFalse);
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
      // With the window and key rows, the most this screen shows.
      final (_, relay) = await pumpApp(tester, [
        deviceJson(capabilities: ['dictate', 'text', 'windows']),
      ], recordEvents: (_) {});
      tester.view.physicalSize = size;
      tester.platformDispatcher.textScaleFactorTestValue = scale;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      await tester.pump();
      expect(tester.takeException(), isNull, reason: '$size idle');
      await tester.ensureVisible(mic);
      await tester.tap(mic);
      await tester.pump();
      await tester.pump();
      expect(find.byTooltip('Discard'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: '$size recording');
      await tester.ensureVisible(find.byTooltip('Discard'));
      await tester.tap(find.byTooltip('Discard'));
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
    await tester.tap(mic);
    await tester.pump();
    await tester.pump();
    expect(find.text('0:00'), findsOneWidget);

    recorder!.success(0); // RecordState.pause
    await tester.pump();
    expect(find.text('Paused'), findsOneWidget);
    recorder!.success(1); // RecordState.record
    await tester.pump();
    expect(find.text('0:00'), findsOneWidget);

    await tester.tap(find.byTooltip('Discard'));
    await finish(tester, relay);
  });

  testWidgets('one page records at a time', (tester) async {
    final (_, relay) = await pumpApp(tester, [
      deviceJson(capabilities: ['dictate', 'text', 'command']),
    ], recordEvents: (_) {});
    await tester.tap(mic);
    await tester.pump();
    await tester.pump();
    await goTo(tester, 'Do');
    expect(
      enabled(tester, find.byIcon(Icons.mic_rounded)),
      isFalse,
      reason: 'Dictate holds the microphone',
    );
    await goTo(tester, 'Dictate');
    await tester.tap(find.byTooltip('Discard'));
    await tester.pump();
    await goTo(tester, 'Do');
    expect(enabled(tester, find.byIcon(Icons.mic_rounded)), isTrue);
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
    expect(hint, findsOneWidget);
    expect(find.byIcon(Icons.expand_more_rounded), findsOneWidget);
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
    expect(mic, findsNothing);

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

    // The top bar switches back.
    await tester.tap(find.text('LAPTOP'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    await tester.tap(find.text('ISHAN-PC'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(prefs.getString('account.dictate.computer'), 'pc');
    expect(hint, findsOneWidget);
    await finish(tester, relay);
  });

  testWidgets('a computer that joined later is never chosen on its own', (
    tester,
  ) async {
    final (server, relay) = await pumpApp(tester, [deviceJson()]);
    expect(hint, findsOneWidget, reason: 'only one');

    await computersAre(tester, server, [
      deviceJson(),
      deviceJson(id: 'new', name: 'NEW-PC'),
    ]);
    expect(find.text('ISHAN-PC'), findsOneWidget, reason: 'still the one');
    expect(hint, findsOneWidget);
    expect(find.byIcon(Icons.expand_more_rounded), findsOneWidget);

    // Once ISHAN-PC leaves the account, NEW-PC is its only computer, and
    // still not chosen for the user.
    await computersAre(tester, server, [deviceJson(id: 'new', name: 'NEW-PC')]);
    expect(find.text('Choose a computer'), findsOneWidget);
    expect(hint, findsNothing);
    await tester.tap(find.text('NEW-PC'));
    await tester.pump();
    expect(hint, findsOneWidget);
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
    await tester.tap(mic);
    await tester.pump();
    await tester.pump();
    await computersAre(tester, server, [
      deviceJson(id: 'other', name: 'OTHER-PC'),
    ]);
    expect(find.text('Choose a computer'), findsNothing);
    expect(find.text('NEW-PC'), findsOneWidget);
    await tester.tap(find.byTooltip('Discard'));
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

  testWidgets('typed text gets no transcription wait: 60 s, then cancels', (
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

    expect(find.text(timedOut), findsOneWidget);
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
    expect(find.text('Reconnecting to the server…'), findsOneWidget);
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
      expect(find.byTooltip('Send'), findsOneWidget, reason: 'still here');
      expect(
        find.textContaining('on it and try again', skipOffstage: false),
        findsOneWidget,
      );
      expect(find.text('Hello'), findsOneWidget);
      expect(find.textContaining('Update Talvio'), findsNothing);
    }
    await finish(tester, relay);
  });

  testWidgets('an online computer without dictation hides Dictate', (
    tester,
  ) async {
    final (_, relay) = await pumpApp(tester, [
      deviceJson(capabilities: ['text']),
    ], page: null);
    expect(find.byType(NavigationBar), findsNothing, reason: 'one page left');
    expect(
      find.text('Update Talvio on ISHAN-PC to see its screen from your phone.'),
      findsOneWidget,
    );
    await tester.tap(find.byTooltip('Account'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Sign out', skipOffstage: false), findsOneWidget);
    await finish(tester, relay);
  });

  group('windows', () {
    final windowsPc = deviceJson(capabilities: ['dictate', 'text', 'windows']);

    /// The body of the last relay the phone sent.
    Map<String, Object?> lastBody(FakeServer server) =>
        server.relays.last['body']! as Map<String, Object?>;

    /// The computer answers request [id] with [result].
    Future<void> answer(
      WidgetTester tester,
      FakeServer server,
      Object? id,
      Map<String, Object?> result,
    ) async {
      server.relay('pc', {'kind': 'result', 'requestId': id, ...result});
      await tester.pump();
      await tester.pump();
    }

    Future<void> tapVisible(WidgetTester tester, Finder finder) async {
      await tester.ensureVisible(finder);
      await tester.pump();
      await tester.tap(finder);
      // A sheet's animation starts on the frame after the tap.
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
    }

    /// Opens the picker; the computer lists Windows Terminal (in front) and
    /// Notepad, and one entry the phone cannot read.
    Future<void> openPicker(WidgetTester tester, FakeServer server) async {
      await tapVisible(tester, find.bySemanticsLabel('Choose window'));
      final body = lastBody(server);
      expect(body['kind'], 'windows.list');
      await answer(tester, server, body['requestId'], {
        'ok': true,
        'windows': [
          {
            'id': 'w1',
            'title': 'Claude Code — zynveliq',
            'app': 'Windows Terminal',
            'front': true,
          },
          {'id': 'w2', 'title': 'notes.txt', 'app': 'Notepad', 'front': false},
          {'id': 3},
        ],
      });
    }

    /// Picks the window of [app] in the open picker.
    Future<void> pick(WidgetTester tester, String app) async {
      await tester.ensureVisible(find.text(app));
      await tester.pump();
      await tester.tap(find.text(app));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
    }

    Future<void> sendVisibleText(WidgetTester tester) async {
      await tester.enterText(find.byType(TextField), 'Hello');
      await tester.pump();
      await tapVisible(tester, find.byTooltip('Send'));
    }

    testWidgets('a computer without them shows nothing about windows', (
      tester,
    ) async {
      final (_, relay) = await pumpApp(tester, [deviceJson()]);
      expect(find.text('Where the cursor is'), findsNothing);
      expect(find.byTooltip('Enter'), findsNothing);
      expect(find.text('Keys'), findsNothing);
      await finish(tester, relay);
    });

    testWidgets('text, Enter and keys go to the chosen window', (tester) async {
      final handle = tester.ensureSemantics();
      final (server, relay) = await pumpApp(tester, [windowsPc]);
      expect(find.text('Where the cursor is'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: 'fits a small phone');

      await openPicker(tester, server);
      expect(find.text('Windows Terminal'), findsOneWidget);
      expect(find.text('Claude Code — zynveliq'), findsOneWidget);
      expect(find.text('In front'), findsOneWidget);
      await pick(tester, 'Notepad');
      expect(find.text('Notepad'), findsOneWidget, reason: 'the row');
      expect(find.text('notes.txt'), findsOneWidget);

      await sendVisibleText(tester);
      final text = lastBody(server);
      expect(text, containsPair('target', 'w2'));
      expect(text, containsPair('stay', false));
      await answer(tester, server, text['requestId'], {
        'ok': true,
        'pasted': true,
      });
      expect(find.text('Sent to Notepad on ISHAN-PC and pasted.'), findsOne);

      await tapVisible(tester, find.text('Press Enter'));
      expect(lastBody(server), {
        'kind': 'windows.keys',
        'keys': ['enter'],
        'target': 'w2',
        'stay': false,
        'requestId': lastBody(server)['requestId'],
      });
      await answer(tester, server, lastBody(server)['requestId'], {
        'ok': true,
        'pressed': 1,
      });
      expect(find.text('Press Enter'), findsNothing, reason: 'used up');

      await tapVisible(tester, find.text('Keys'));
      await tapVisible(tester, find.byTooltip('Tab'));
      expect(lastBody(server)['keys'], ['tab']);
      expect(lastBody(server)['target'], 'w2');
      await answer(tester, server, lastBody(server)['requestId'], {'ok': true});
      await tapVisible(tester, find.bySemanticsLabel('Escape'));
      expect(lastBody(server)['keys'], ['escape']);
      await answer(tester, server, lastBody(server)['requestId'], {'ok': true});
      await tapVisible(tester, find.byTooltip('More keys'));
      await tester.tap(find.text('Ctrl + C (copy)'));
      await tester.pump(const Duration(milliseconds: 500));
      expect(lastBody(server)['keys'], ['ctrl+c']);

      await finish(tester, relay);
      handle.dispose();
    });

    testWidgets('a dictation carries the chosen window too', (tester) async {
      final file = File(
        '${Directory.systemTemp.createTempSync('talvio-test').path}/d.wav',
      )..writeAsBytesSync(List.filled(44, 0));
      final handle = tester.ensureSemantics();
      final (server, relay) = await pumpApp(
        tester,
        [windowsPc],
        recordEvents: (_) {},
        recording: file.path,
      );
      await openPicker(tester, server);
      await pick(tester, 'Windows Terminal');

      await tapVisible(tester, mic);
      await tester.pump();
      expect(
        tester
            .widget<InkWell>(
              find.descendant(
                of: find.bySemanticsLabel('Choose window'),
                matching: find.byType(InkWell),
              ),
            )
            .onTap,
        isNull,
        reason: 'a recording keeps its window',
      );
      await tapVisible(tester, stop);
      // The audio is encoded on another isolate.
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 300)),
      );
      await tester.pump();
      final body = lastBody(server);
      expect(body['kind'], 'dictate');
      expect(body['target'], 'w1');
      expect(body['stay'], false);
      await answer(tester, server, body['requestId'], {
        'ok': true,
        'text': 'ls',
        'pasted': true,
      });
      // The sent recording is deleted from disk before the outcome shows.
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 100)),
      );
      await tester.pump();
      expect(find.text('Pasted in Windows Terminal on ISHAN-PC'), findsOne);
      expect(find.text('Press Enter'), findsOneWidget);
      await tester.pump(const Duration(seconds: 9));
      expect(find.text('Press Enter'), findsNothing, reason: 'it goes');

      // The dictation is listed under Recent, to open again.
      expect(find.text('Recent', skipOffstage: false), findsOneWidget);
      final row = find.text('ls', skipOffstage: false).last;
      await tester.ensureVisible(row);
      await tester.pump();
      await tester.tap(row);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      expect(find.text('Paste on ISHAN-PC'), findsOneWidget);
      await finish(tester, relay);
      handle.dispose();
    });

    testWidgets('a window that closed is forgotten and can be chosen again', (
      tester,
    ) async {
      final handle = tester.ensureSemantics();
      final (server, relay) = await pumpApp(tester, [windowsPc]);
      await openPicker(tester, server);
      await pick(tester, 'Notepad');

      await sendVisibleText(tester);
      await answer(tester, server, lastBody(server)['requestId'], {
        'ok': false,
        'error': 'target_gone',
      });
      expect(find.textContaining('That window closed on ISHAN-PC'), findsOne);
      expect(
        find.text('Where the cursor is', skipOffstage: false),
        findsOneWidget,
      );
      expect(find.text('Hello'), findsOneWidget, reason: 'text kept');

      await tapVisible(
        tester,
        find.widgetWithText(TextButton, 'Choose window'),
      );
      expect(lastBody(server)['kind'], 'windows.list');
      await finish(tester, relay);
      handle.dispose();
    });

    testWidgets('a new list moves the chosen window to its new id', (
      tester,
    ) async {
      final handle = tester.ensureSemantics();
      final (server, relay) = await pumpApp(tester, [windowsPc]);
      await openPicker(tester, server);
      await pick(tester, 'Notepad');

      await tapVisible(tester, find.bySemanticsLabel('Choose window'));
      await answer(tester, server, lastBody(server)['requestId'], {
        'ok': true,
        'windows': [
          {'id': 'n7', 'title': 'notes.txt', 'app': 'Notepad', 'front': true},
        ],
      });
      // Closed without a choice: the same window, under its new id.
      await tester.tapAt(const Offset(10, 10));
      await tester.pump(const Duration(milliseconds: 500));
      await tapVisible(tester, find.byTooltip('Enter'));
      expect(lastBody(server)['target'], 'n7');
      await finish(tester, relay);
      handle.dispose();
    });

    testWidgets('keys wait for the press before them and go together', (
      tester,
    ) async {
      final (server, relay) = await pumpApp(tester, [windowsPc]);
      final enter = find.byTooltip('Enter');
      await tester.ensureVisible(enter);
      await tester.pump();
      for (var i = 0; i < 5; i++) {
        await tester.tap(enter);
        await tester.pump();
      }
      List<Object?> keyRequests() => [
        for (final relay in server.relays)
          if ((relay['body']! as Map)['kind'] == 'windows.keys') relay['body'],
      ];
      // One request at a time: the computer has one slot for phone input.
      expect(keyRequests(), hasLength(1));
      expect((keyRequests().first! as Map).containsKey('target'), isFalse);
      expect((keyRequests().first! as Map)['keys'], ['enter']);

      await answer(tester, server, lastBody(server)['requestId'], {
        'ok': true,
        'pressed': 1,
      });
      expect(keyRequests(), hasLength(2));
      expect((keyRequests().last! as Map)['keys'], [
        'enter',
        'enter',
        'enter',
        'enter',
      ]);

      await answer(tester, server, lastBody(server)['requestId'], {
        'ok': false,
        'error': 'remote_windows_off',
      });
      expect(
        find.textContaining('Turn on "Let phones see and use windows"'),
        findsOneWidget,
      );
      await finish(tester, relay);
    });

    testWidgets('the picker says when the computer has it turned off', (
      tester,
    ) async {
      final handle = tester.ensureSemantics();
      final (server, relay) = await pumpApp(tester, [windowsPc]);
      await tapVisible(tester, find.bySemanticsLabel('Choose window'));
      await answer(tester, server, lastBody(server)['requestId'], {
        'ok': false,
        'error': 'remote_windows_off',
      });
      expect(
        find.textContaining('Turn on "Let phones see and use windows"'),
        findsOneWidget,
      );
      expect(find.text('Where the cursor is'), findsNWidgets(2));
      await finish(tester, relay);
      handle.dispose();
    });

    group('live view', () {
      final screenPc = deviceJson(
        capabilities: ['dictate', 'text', 'windows', 'screen'],
      );

      /// Opens the picker; the computer lists Notepad and one screen.
      Future<void> listWithScreen(
        WidgetTester tester,
        FakeServer server,
      ) async {
        await tapVisible(tester, find.bySemanticsLabel('Choose window'));
        await answer(tester, server, lastBody(server)['requestId'], {
          'ok': true,
          'windows': [
            {'id': 'w2', 'title': 'notes.txt', 'app': 'Notepad', 'front': true},
          ],
          'screens': [
            {
              'id': 's3',
              'name': 'Display 1',
              'width': 2560,
              'height': 1440,
              'primary': true,
            },
          ],
        });
      }

      testWidgets('a computer without `screen` offers no Watch', (
        tester,
      ) async {
        final handle = tester.ensureSemantics();
        final (server, relay) = await pumpApp(tester, [windowsPc]);
        await listWithScreen(tester, server);
        expect(find.text('Notepad'), findsOneWidget);
        expect(find.text('Screens'), findsNothing);
        expect(find.text('Display 1'), findsNothing);
        expect(find.byTooltip('Watch Notepad'), findsNothing);
        await pick(tester, 'Notepad');
        expect(find.byTooltip('Show the screen'), findsNothing);
        await finish(tester, relay);
        handle.dispose();
      });

      testWidgets('lists screens, and watches a window from the picker', (
        tester,
      ) async {
        final handle = tester.ensureSemantics();
        final (server, relay) = await pumpApp(tester, [screenPc]);
        await listWithScreen(tester, server);
        expect(find.text('Screens'), findsOneWidget);
        expect(find.text('Display 1'), findsOneWidget);
        expect(find.text('2560 × 1440 · Main'), findsOneWidget);
        expect(find.byTooltip('Watch Display 1'), findsOneWidget);

        await tapVisible(tester, find.byTooltip('Watch Notepad'));
        expect(lastBody(server), {
          'kind': 'screen.watch',
          'target': 'w2',
          'control': false,
          'audio': false,
          'requestId': lastBody(server)['requestId'],
        });
        // The small view, at the top of the page.
        expect(find.byTooltip('Stop watching'), findsOneWidget);
        expect(find.text('Starting…'), findsWidgets);
        expect(tester.takeException(), isNull, reason: 'fits a small phone');

        final id = lastBody(server)['requestId'];
        await tester.tap(find.byTooltip('Stop watching'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));
        expect(lastBody(server), {'kind': 'cancel', 'requestId': id});
        expect(find.byTooltip('Stop watching'), findsNothing);

        // Watching aimed dictation at Notepad.
        expect(find.text('notes.txt'), findsOneWidget);
        await finish(tester, relay);
        handle.dispose();
      });
    });
  });

  group('navigation and the Screen page', () {
    final screenPc = deviceJson(
      capabilities: ['dictate', 'text', 'windows', 'screen'],
    );
    final listing = {
      'ok': true,
      'windows': [
        {'id': 'w2', 'title': 'notes.txt', 'app': 'Notepad', 'front': true},
        {'id': 'w5', 'title': 'Inbox', 'app': 'Outlook', 'front': false},
      ],
      'screens': [
        {
          'id': 's4',
          'name': 'Display 2',
          'width': 1920,
          'height': 1080,
          'primary': false,
        },
        {
          'id': 's3',
          'name': 'Display 1',
          'width': 2560,
          'height': 1440,
          'primary': true,
        },
      ],
    };

    Map<String, Object?> lastBody(FakeServer server) =>
        server.relays.last['body']! as Map<String, Object?>;

    /// Answers the latest `windows.list` with [listing].
    Future<void> answerList(WidgetTester tester, FakeServer server) async {
      final body = server.relays
          .map((r) => r['body']! as Map<String, Object?>)
          .lastWhere((body) => body['kind'] == 'windows.list');
      server.relay('pc', {
        'kind': 'result',
        'requestId': body['requestId'],
        ...listing,
      });
      await tester.pump();
      await tester.pump();
    }

    testWidgets('the bar lists what the computer offers; Ask is home', (
      tester,
    ) async {
      final (server, relay) = await pumpApp(tester, [deviceJson()], page: null);
      expect(tabs(tester), ['Dictate', 'Screen']);
      expect(find.text('ISHAN-PC'), findsOneWidget);
      expect(find.text('Online · Windows'), findsOneWidget);
      expect(tester.takeException(), isNull, reason: 'fits a small phone');

      await goTo(tester, 'Screen');
      expect(find.text("See ISHAN-PC's screen here"), findsOneWidget);
      // Back goes home before it leaves the app.
      await tester.binding.handlePopRoute();
      await tester.pump();
      expect(find.text("See ISHAN-PC's screen here"), findsNothing);
      expect(hint, findsOneWidget, reason: 'home');

      await computersAre(tester, server, [
        deviceJson(
          capabilities: ['dictate', 'text', 'command', 'assistant', 'windows'],
        ),
      ]);
      expect(tabs(tester), ['Ask', 'Do', 'Dictate', 'Screen']);
      await finish(tester, relay);
    });

    testWidgets('Screen lists windows and screens, and opens live view', (
      tester,
    ) async {
      final handle = tester.ensureSemantics();
      final (server, relay) = await pumpApp(tester, [screenPc], page: 'Screen');
      expect(
        find.text('Looking for windows and screens…', skipOffstage: false),
        findsOneWidget,
      );
      await answerList(tester, server);
      expect(tester.takeException(), isNull, reason: 'fits a small phone');
      expect(find.text('View screen'), findsOneWidget);
      expect(find.text('Display 1 · 2560 × 1440'), findsOneWidget);
      await tester.scrollUntilVisible(find.text('Notepad'), 100);
      expect(find.text('Windows'), findsOneWidget);
      expect(find.text('In front'), findsOneWidget);
      await tester.scrollUntilVisible(find.text('Display 2'), 100);
      expect(find.text('1920 × 1080'), findsOneWidget);

      // View screen opens the main one, not the first listed.
      await tester.ensureVisible(find.text('View screen', skipOffstage: false));
      await tester.pump();
      await tester.tap(find.text('View screen'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      expect(lastBody(server)['kind'], 'screen.watch');
      expect(lastBody(server)['target'], 's3');
      expect(find.byTooltip('Stop watching'), findsOneWidget);
      final watch = lastBody(server)['requestId'];
      await tester.tap(find.byTooltip('Stop watching'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      expect(
        server.relays.map((r) => r['body']),
        anyElement(equals({'kind': 'cancel', 'requestId': watch})),
      );

      // A window opens straight into live view, and aims input at it.
      await tester.ensureVisible(find.text('Outlook', skipOffstage: false));
      await tester.pump();
      await tester.tap(find.text('Outlook'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      expect(lastBody(server)['kind'], 'screen.watch');
      expect(lastBody(server)['target'], 'w5');
      // The view stays while the user goes to another page.
      await goTo(tester, 'Dictate');
      expect(find.byTooltip('Stop watching'), findsOneWidget);
      expect(find.text('Outlook'), findsNWidgets(2), reason: 'target, view');
      await tester.tap(find.byTooltip('Stop watching'));
      await tester.pump();
      await finish(tester, relay);
      handle.dispose();
    });

    testWidgets('the top bar shows the screen with one tap', (tester) async {
      final (server, relay) = await pumpApp(tester, [screenPc]);
      await tester.tap(find.byTooltip('Show the screen'));
      await tester.pump();
      expect(lastBody(server)['kind'], 'windows.list');
      await answerList(tester, server);
      await tester.pump();
      expect(lastBody(server)['kind'], 'screen.watch');
      expect(lastBody(server)['target'], 's3');
      expect(find.byTooltip('Hide the screen'), findsOneWidget);
      await tester.tap(find.byTooltip('Hide the screen'));
      await tester.pump();
      expect(lastBody(server)['kind'], 'cancel');
      await finish(tester, relay);
    });

    testWidgets('Screen says how to turn on seeing the screen', (tester) async {
      final (server, relay) = await pumpApp(tester, [
        deviceJson(capabilities: ['dictate', 'text', 'windows']),
      ], page: 'Screen');
      expect(find.text("See ISHAN-PC's screen here"), findsOneWidget);
      expect(
        find.textContaining('"Let phones see and use windows"'),
        findsWidgets,
      );
      expect(find.text('View screen'), findsNothing);
      expect(server.relays, isEmpty, reason: 'nothing to list');
      await finish(tester, relay);
    });

    testWidgets('Screen with the computer offline says to open Talvio', (
      tester,
    ) async {
      final (_, relay) = await pumpApp(tester, [
        deviceJson(online: false),
      ], page: 'Screen');
      expect(
        find.text('Open Talvio on ISHAN-PC to see its screen.'),
        findsOneWidget,
      );
      await finish(tester, relay);
    });

    testWidgets('a wide window shows the pages in a rail', (tester) async {
      final (_, relay) = await pumpApp(tester, [deviceJson()], page: null);
      tester.view.physicalSize = const Size(800, 400);
      await tester.pump();
      expect(find.byType(NavigationRail), findsOneWidget);
      expect(find.byType(NavigationBar), findsNothing);
      expect(tester.takeException(), isNull);
      await goTo(tester, 'Screen');
      expect(find.text("See ISHAN-PC's screen here"), findsOneWidget);
      await finish(tester, relay);
    });
  });

  testWidgets('account lists devices and confirms a removal', (tester) async {
    final (_, relay) = await pumpApp(tester, [deviceJson(online: false)]);
    await tester.tap(find.byTooltip('Account'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));

    expect(find.text('This phone'), findsOneWidget);
    expect(find.text('Ishan'), findsOneWidget);
    expect(find.textContaining('Last seen'), findsOneWidget);
    expect(find.text('Development server · sync.test:8787'), findsOneWidget);

    await tester.tap(find.byTooltip('Remove ISHAN-PC'));
    await tester.pumpAndSettle();
    expect(find.text('Remove ISHAN-PC?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.text('Remove ISHAN-PC?'), findsNothing);
    await finish(tester, relay);
  });
}
