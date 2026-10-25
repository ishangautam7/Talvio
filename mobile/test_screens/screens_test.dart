// Renders every screen to PNG, light and dark, with the real fonts, for
// design review. Not part of `flutter test`; run it on its own:
//
//   flutter test test_screens --dart-define=OUT=/some/folder
//
// (OUT defaults to build/screens.)

// ignore_for_file: invalid_use_of_visible_for_testing_member

import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:talvio_mobile/src/app.dart';
import 'package:talvio_mobile/src/core/api_client.dart';
import 'package:talvio_mobile/src/core/relay_client.dart';
import 'package:talvio_mobile/src/design/components.dart';
import 'package:talvio_mobile/src/features/screen/live_controller.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/rtc_link.dart';

import '../test/helpers.dart';

const out = String.fromEnvironment('OUT', defaultValue: 'build/screens');

final _root = GlobalKey();

Future<void> _loadFonts() async {
  Future<void> family(String name, List<String> assets) async {
    final loader = FontLoader(name);
    for (final asset in assets) {
      loader.addFont(rootBundle.load(asset));
    }
    await loader.load();
  }

  await family('Inter', [
    for (final w in [400, 500, 600, 700]) 'assets/fonts/Inter-$w.ttf',
  ]);
  await family('PlusJakartaSans', [
    'assets/fonts/PlusJakartaSans-700.ttf',
    'assets/fonts/PlusJakartaSans-800.ttf',
  ]);
  // Flutter's own copy of the icon font: tests only have Ahem otherwise.
  final root = Platform.environment['FLUTTER_ROOT'] ?? '.';
  final icons = FontLoader('MaterialIcons')
    ..addFont(
      Future.value(
        ByteData.sublistView(
          File(
            '$root/bin/cache/artifacts/material_fonts/materialicons-regular.otf',
          ).readAsBytesSync(),
        ),
      ),
    );
  await icons.load();
}

Future<void> shot(WidgetTester tester, String name) async {
  await tester.pump(const Duration(milliseconds: 600));
  await tester.pump(const Duration(milliseconds: 600));
  final boundary =
      _root.currentContext!.findRenderObject()! as RenderRepaintBoundary;
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 2);
    final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
    Directory(out).createSync(recursive: true);
    File('$out/$name.png').writeAsBytesSync(bytes!.buffer.asUint8List());
  });
}

/// A painted stand-in for the computer's screen.
class FakeDesktop extends StatelessWidget {
  const FakeDesktop({super.key});

  @override
  Widget build(BuildContext context) =>
      const CustomPaint(painter: _DesktopPainter(), child: SizedBox.expand());
}

class _DesktopPainter extends CustomPainter {
  const _DesktopPainter();

  @override
  void paint(Canvas canvas, Size size) {
    final r = Offset.zero & size;
    canvas.drawRect(
      r,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [Color(0xFF243B6B), Color(0xFF6B3B7A)],
        ).createShader(r),
    );
    final win = Rect.fromLTWH(
      size.width * 0.08,
      size.height * 0.08,
      size.width * 0.62,
      size.height * 0.74,
    );
    canvas.drawRRect(
      RRect.fromRectAndRadius(win, const Radius.circular(6)),
      Paint()..color = const Color(0xFFF7F7F9),
    );
    canvas.drawRect(
      Rect.fromLTWH(win.left, win.top, win.width, size.height * 0.07),
      Paint()..color = const Color(0xFFE8E8EE),
    );
    final red = Rect.fromLTWH(
      win.left + win.width * 0.06,
      win.top + size.height * 0.12,
      win.width * 0.88,
      win.height * 0.42,
    );
    canvas.drawRect(red, Paint()..color = const Color(0xFF111111));
    final play = Path()
      ..moveTo(red.center.dx - 10, red.center.dy - 12)
      ..lineTo(red.center.dx + 14, red.center.dy)
      ..lineTo(red.center.dx - 10, red.center.dy + 12)
      ..close();
    canvas.drawPath(play, Paint()..color = const Color(0xFFFF0033));
    final line = Paint()..color = const Color(0xFFCFCFD8);
    for (var i = 0; i < 4; i++) {
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromLTWH(
            red.left,
            red.bottom + 10 + i * 12.0,
            red.width * (0.9 - i * 0.18),
            6,
          ),
          const Radius.circular(3),
        ),
        line,
      );
    }
    final side = Rect.fromLTWH(
      size.width * 0.74,
      size.height * 0.08,
      size.width * 0.2,
      size.height * 0.5,
    );
    canvas.drawRRect(
      RRect.fromRectAndRadius(side, const Radius.circular(6)),
      Paint()..color = const Color(0xFF1E1F2E),
    );
    canvas.drawRect(
      Rect.fromLTWH(0, size.height * 0.93, size.width, size.height * 0.07),
      Paint()..color = const Color(0xCC111222),
    );
  }

  @override
  bool shouldRepaint(_DesktopPainter oldDelegate) => false;
}

class _Link implements RtcLink {
  _Link(this.events);

  final RtcLinkEvents events;

  @override
  Future<String> answer(String offerSdp) async => 'v=0 answer';

  @override
  Future<void> addCandidate(IceCandidate candidate) async {}

  @override
  bool send(String text) => true;

  @override
  set soundOn(bool on) {}

  @override
  ValueNotifier<Size?> get videoSize => _size;
  final _size = ValueNotifier<Size?>(const Size(1920, 1080));

  @override
  Widget buildVideo() => const FakeDesktop();

  @override
  Future<void> close() async {}
}

const _computer = [
  'dictate',
  'text',
  'windows',
  'screen',
  'command',
  'assistant',
];

Future<(FakeServer, RelayClient)> app(
  WidgetTester tester, {
  bool dark = false,
  Map<String, String> prefs = const {},
  List<Map<String, Object?>>? devices,
}) async {
  ambientMotion = false;
  debugDisableShadows = false;
  tester.view
    ..physicalSize = const Size(393, 852)
    ..devicePixelRatio = 1;
  tester.platformDispatcher.platformBrightnessTestValue = dark
      ? Brightness.dark
      : Brightness.light;
  addTearDown(tester.view.reset);
  addTearDown(tester.platformDispatcher.clearPlatformBrightnessTestValue);
  final messenger = tester.binding.defaultBinaryMessenger;
  messenger.setMockMethodCallHandler(
    const MethodChannel('plugins.flutter.io/path_provider'),
    (_) async => '/nonexistent-talvio-test',
  );
  final random = math.Random(4);
  messenger.setMockMethodCallHandler(
    const MethodChannel('com.llfbandit.record/messages'),
    (call) async {
      if (call.method == 'create') {
        final id = (call.arguments as Map)['recorderId'];
        messenger.setMockStreamHandler(
          EventChannel('com.llfbandit.record/events/$id'),
          MockStreamHandler.inline(onListen: (_, sink) {}),
        );
      }
      return switch (call.method) {
        'hasPermission' => true,
        'isRecording' => true,
        'getAmplitude' => {
          'current': -45 + random.nextDouble() * 42,
          'max': -3.0,
        },
        _ => null,
      };
    },
  );
  debugLinkFactory = (servers, events) async => _Link(events);
  addTearDown(() => debugLinkFactory = null);

  final session = await sessionStore();
  final shared = await SharedPreferences.getInstance();
  for (final MapEntry(:key, :value) in prefs.entries) {
    await shared.setString(key, value);
  }
  final server = FakeServer();
  final relay = RelayClient(session, connectChannel: server.connect);
  await tester.pumpWidget(
    RepaintBoundary(
      key: _root,
      child: TalvioApp(
        session: session,
        api: ApiClient(
          session,
          httpClient: MockClient((_) async => http.Response('', 204)),
        ),
        relay: relay,
        prefs: shared,
      ),
    ),
  );
  await tester.pump();
  server
    ..send({'type': 'ready', 'deviceId': 'phone', 'user': userJson()})
    ..send({
      'type': 'devices',
      'devices':
          devices ??
          [
            deviceJson(
              id: 'phone',
              name: 'Pixel 9',
              kind: 'mobile',
              current: true,
            ),
            deviceJson(capabilities: _computer),
          ],
    });
  await tester.pump();
  await tester.pump();
  return (server, relay);
}

Future<void> finish(WidgetTester tester, RelayClient relay) async {
  await tester.pumpWidget(const SizedBox());
  relay.dispose();
  debugDisableShadows = true;
}

Future<void> tab(WidgetTester tester, String label) async {
  await tester.tap(
    find.descendant(of: find.byType(NavigationBar), matching: find.text(label)),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 400));
}

String lastId(FakeServer server, String kind) {
  final body = server.relays
      .map((r) => r['body']! as Map<String, Object?>)
      .lastWhere((b) => b['kind'] == kind);
  return body['requestId']! as String;
}

void reply(FakeServer server, String id, Map<String, Object?> body) =>
    server.relay('pc', {'requestId': id, ...body});

const chat = '''
{"conversation":"c1","conversationComputer":"pc","turns":[
{"computerId":"pc","computerName":"ISHAN-PC","spoken":true,
 "question":"Play Believer by Imagine Dragons on YouTube",
 "answer":"Playing **Believer** by Imagine Dragons on YouTube. Turn it up from the dock if you want to hear it here.",
 "actions":[{"label":"Opened YouTube in Chrome","status":"done"},{"label":"Played “Believer”","status":"done"}]},
{"computerId":"pc","computerName":"ISHAN-PC","spoken":false,
 "question":"Email Ron that the meeting moves to 9",
 "answer":"I drafted it in Gmail:\\n\\n> Hi Ron, the meeting moves to **9:00** tomorrow. See you then.",
 "actions":[{"label":"Drafted an email to Ron","status":"done"},{"label":"Send the email to Ron","status":"awaiting_confirmation"}],
 "pending":{"id":7,"summary":"Send the email to Ron"}}
]}''';

final commandsRecent =
    '[{"said":"Open YouTube","feedback":"Opened YouTube.","status":"done","at":${DateTime(2026, 10, 25, 9, 12).millisecondsSinceEpoch}},'
    '{"said":"Turn the volume down","feedback":"Volume set to 30%.","status":"done","at":${DateTime(2026, 10, 25, 9, 2).millisecondsSinceEpoch}},'
    '{"said":"Open Spotfy","feedback":"I couldn\'t find an app called Spotfy.","status":"failed","at":${DateTime(2026, 10, 24, 18, 40).millisecondsSinceEpoch}}]';

final dictationsRecent =
    '[{"text":"Thanks for the notes. I will send the revised deck by Friday afternoon.","at":${DateTime(2026, 10, 25, 9, 30).millisecondsSinceEpoch}},'
    '{"text":"Can we move the stand-up to ten tomorrow?","at":${DateTime(2026, 10, 24, 17, 5).millisecondsSinceEpoch}}]';

void main() {
  setUpAll(_loadFonts);

  for (final dark in [false, true]) {
    final m = dark ? 'dark' : 'light';

    testWidgets('sign in $m', (tester) async {
      ambientMotion = false;
      debugDisableShadows = false;
      tester.view
        ..physicalSize = const Size(393, 852)
        ..devicePixelRatio = 1;
      tester.platformDispatcher.platformBrightnessTestValue = dark
          ? Brightness.dark
          : Brightness.light;
      addTearDown(tester.view.reset);
      final session = await sessionStore(signedIn: false);
      final relay = RelayClient(session, connectChannel: FakeServer().connect);
      await tester.pumpWidget(
        RepaintBoundary(
          key: _root,
          child: TalvioApp(
            session: session,
            api: ApiClient(
              session,
              httpClient: MockClient(
                (_) async => http.Response('{"google":true}', 200),
              ),
            ),
            relay: relay,
            prefs: await SharedPreferences.getInstance(),
          ),
        ),
      );
      await tester.pump();
      await shot(tester, '01-sign-in-$m');
      await finish(tester, relay);
    });

    testWidgets('ask empty $m', (tester) async {
      final (_, relay) = await app(tester, dark: dark);
      await shot(tester, '02-ask-empty-$m');
      await finish(tester, relay);
    });

    testWidgets('ask chat $m', (tester) async {
      final (server, relay) = await app(
        tester,
        dark: dark,
        prefs: {'account.assistant.chat': chat},
      );
      await tester.enterText(
        find.byType(TextField),
        "What's on my calendar tomorrow?",
      );
      await tester.pump();
      await tester.tap(find.byTooltip('Send'));
      await tester.pump();
      final id = lastId(server, 'assistant.ask');
      reply(server, id, {
        'kind': 'progress',
        'stage': 'tool',
        'label': 'Reading your calendar',
      });
      await tester.pump();
      await tester.pump();
      await shot(tester, '03-ask-chat-$m');
      reply(server, id, {
        'kind': 'progress',
        'stage': 'answer',
        'text':
            'Tomorrow you have:\n\n- **9:00** Stand-up with the design team\n- **13:30** Lunch with Ron',
      });
      await tester.pump();
      await tester.pump();
      await shot(tester, '04-ask-streaming-$m');
      await finish(tester, relay);
    });

    testWidgets('do $m', (tester) async {
      final (server, relay) = await app(
        tester,
        dark: dark,
        prefs: {'account.command.recent': commandsRecent},
      );
      await tab(tester, 'Do');
      await shot(tester, '05-do-$m');
      await tester.enterText(
        find.byType(TextField),
        'Email Ron that the meeting moves to 9',
      );
      await tester.pump();
      await tester.tap(find.byTooltip('Send the command'));
      await tester.pump();
      final id = lastId(server, 'command');
      reply(server, id, {'kind': 'progress', 'stage': 'approval'});
      await tester.pump();
      await tester.pump();
      await shot(tester, '06-do-approval-$m');
      reply(server, id, {'kind': 'progress', 'stage': 'working'});
      await tester.pump();
      await tester.pump();
      await shot(tester, '07-do-working-$m');
      reply(server, id, {
        'kind': 'result',
        'ok': true,
        'transcript': 'Email Ron that the meeting moves to 9',
        'status': 'awaiting_confirmation',
        'feedback': 'I drafted the email to Ron in Gmail.',
        'id': 7,
        'pending': 'Send the email to Ron',
      });
      await tester.pump();
      await tester.pump();
      await shot(tester, '08-do-confirm-$m');
      await finish(tester, relay);
    });

    testWidgets('dictate $m', (tester) async {
      final (server, relay) = await app(
        tester,
        dark: dark,
        prefs: {'account.dictate.recent': dictationsRecent},
      );
      await tab(tester, 'Dictate');
      await shot(tester, '09-dictate-$m');
      await tester.tap(find.bySemanticsLabel('Start dictating'));
      for (var i = 0; i < 30; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      await shot(tester, '10-dictate-recording-$m');
      await tester.tap(find.byTooltip('Discard'));
      await tester.pump();
      await tester.enterText(find.byType(TextField), 'See you at nine.');
      await tester.pump();
      await tester.tap(find.byTooltip('Send'));
      await tester.pump();
      final id = lastId(server, 'text');
      reply(server, id, {'kind': 'result', 'ok': true});
      await tester.pump();
      await tester.pump();
      FocusManager.instance.primaryFocus?.unfocus();
      await tester.pump();
      await shot(tester, '11-dictate-pasted-$m');
      await finish(tester, relay);
    });

    testWidgets('screen and live $m', (tester) async {
      final (server, relay) = await app(tester, dark: dark);
      await tab(tester, 'Screen');
      final id = lastId(server, 'windows.list');
      reply(server, id, {
        'kind': 'result',
        'ok': true,
        'windows': [
          {
            'id': 'w1',
            'title': 'Believer - YouTube',
            'app': 'Google Chrome',
            'front': true,
          },
          {
            'id': 'w2',
            'title': 'Claude Code — zynveliq',
            'app': 'Windows Terminal',
          },
          {'id': 'w3', 'title': 'Q4 plan.docx', 'app': 'Word'},
          {'id': 'w4', 'title': 'Inbox (3)', 'app': 'Outlook'},
        ],
        'screens': [
          {
            'id': 's1',
            'name': 'Display 1',
            'width': 2560,
            'height': 1440,
            'primary': true,
          },
        ],
      });
      await tester.pump();
      await tester.pump();
      await shot(tester, '12-screen-$m');
      await tester.tap(find.text('View screen'));
      await tester.pump();
      final watch = lastId(server, 'screen.watch');
      reply(server, watch, {
        'kind': 'progress',
        'stage': 'rtc.offer',
        'sdp': 'v=0 offer',
        'iceServers': const [],
      });
      await tester.pump();
      await tester.pump();
      reply(server, watch, {'kind': 'progress', 'stage': 'live'});
      await tester.pump();
      await tester.pump();
      await shot(tester, '13-screen-live-$m');
      await tab(tester, 'Ask');
      await shot(tester, '14-ask-with-live-$m');
      await tester.tap(find.byTooltip('Full screen'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      await shot(tester, '15-live-full-$m');
      await finish(tester, relay);
    });

    testWidgets('account $m', (tester) async {
      final (_, relay) = await app(
        tester,
        dark: dark,
        devices: [
          deviceJson(
            id: 'phone',
            name: 'Pixel 9',
            kind: 'mobile',
            current: true,
          ),
          deviceJson(capabilities: _computer),
          deviceJson(id: 'mac', name: 'MacBook Air', online: false),
        ],
      );
      await tester.tap(find.byTooltip('Account'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 500));
      await shot(tester, '16-account-$m');
      await finish(tester, relay);
    });

    testWidgets('states $m', (tester) async {
      final (server, relay) = await app(
        tester,
        dark: dark,
        devices: [
          deviceJson(
            id: 'phone',
            name: 'Pixel 9',
            kind: 'mobile',
            current: true,
          ),
          deviceJson(online: false),
        ],
      );
      await shot(tester, '17-offline-$m');
      server.send({
        'type': 'devices',
        'devices': [deviceJson(id: 'phone', kind: 'mobile', current: true)],
      });
      await tester.pump();
      await tester.pump();
      await shot(tester, '18-no-computer-$m');
      await finish(tester, relay);
    });
  }
}
