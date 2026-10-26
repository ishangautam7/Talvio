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
import 'package:talvio_mobile/src/shell/home_shell.dart';

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

  await family('Geist', [
    for (final w in ['Regular', 'Medium', 'SemiBold'])
      'assets/fonts/Geist-$w.ttf',
  ]);
  await family('GeistMono', [
    'assets/fonts/GeistMono-Regular.ttf',
    'assets/fonts/GeistMono-Medium.ttf',
  ]);
  await family('PlusJakartaSans', ['assets/fonts/PlusJakartaSans-800.ttf']);
  for (final style in ['Regular', 'Fill', 'Duotone']) {
    await family('Phosphor$style', ['assets/fonts/Phosphor-$style.ttf']);
  }
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

  static void _text(
    Canvas canvas,
    String text,
    Offset at,
    double size,
    Color color, {
    FontWeight weight = FontWeight.w400,
    double? maxWidth,
  }) {
    final painter = TextPainter(
      text: TextSpan(
        text: text,
        style: TextStyle(
          fontFamily: 'Geist',
          fontSize: size,
          fontWeight: weight,
          color: color,
          height: 1.2,
        ),
      ),
      textDirection: TextDirection.ltr,
      maxLines: 1,
      ellipsis: '…',
    )..layout(maxWidth: maxWidth ?? double.infinity);
    painter.paint(canvas, at);
    painter.dispose();
  }

  static void _box(Canvas canvas, Rect r, Color color, [double radius = 0]) =>
      canvas.drawRRect(
        RRect.fromRectAndRadius(r, Radius.circular(radius)),
        Paint()..color = color,
      );

  @override
  void paint(Canvas canvas, Size size) {
    final w = size.width;
    final h = size.height;
    final all = Offset.zero & size;
    // Wallpaper: a Windows-style bloom.
    canvas.drawRect(
      all,
      Paint()
        ..shader = const RadialGradient(
          center: Alignment(0.2, 0.4),
          radius: 1.1,
          colors: [Color(0xFF4F7BD9), Color(0xFF1B3A86), Color(0xFF0C1736)],
        ).createShader(all),
    );

    // The browser window.
    final win = Rect.fromLTWH(w * 0.035, h * 0.04, w * 0.93, h * 0.86);
    canvas.save();
    canvas.clipRRect(RRect.fromRectAndRadius(win, const Radius.circular(8)));
    _box(canvas, win, const Color(0xFF0F0F0F));
    final u = win.width / 100; // one percent of the window's width
    // Tab strip and address bar.
    _box(
      canvas,
      Rect.fromLTWH(win.left, win.top, win.width, u * 3.6),
      const Color(0xFF202124),
    );
    _box(
      canvas,
      Rect.fromLTWH(win.left + u * 1.5, win.top + u * 0.8, u * 16, u * 2.8),
      const Color(0xFF35363A),
      u * 0.8,
    );
    _box(
      canvas,
      Rect.fromLTWH(win.left + u * 2.3, win.top + u * 1.6, u * 1.2, u * 1.2),
      const Color(0xFFFF0033),
      u * 0.3,
    );
    _text(
      canvas,
      'Believer - YouTube',
      Offset(win.left + u * 4.2, win.top + u * 1.25),
      u * 1.25,
      const Color(0xFFE8EAED),
      maxWidth: u * 12,
    );
    _box(
      canvas,
      Rect.fromLTWH(win.left, win.top + u * 3.6, win.width, u * 3.4),
      const Color(0xFF35363A),
    );
    _box(
      canvas,
      Rect.fromLTWH(win.left + u * 8, win.top + u * 4.2, u * 70, u * 2.2),
      const Color(0xFF202124),
      u * 1.1,
    );
    _text(
      canvas,
      'youtube.com/watch?v=7wtfhZwyrcc',
      Offset(win.left + u * 9.5, win.top + u * 4.55),
      u * 1.2,
      const Color(0xFFBDC1C6),
    );

    // The player: warm stage light, the band on it.
    final top = win.top + u * 9;
    final player = Rect.fromLTWH(win.left + u * 3, top, u * 64, u * 36);
    canvas.drawRect(
      player,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [Color(0xFF2B0F3A), Color(0xFFB2331F), Color(0xFFF59E3B)],
          stops: [0.0, 0.6, 1.0],
        ).createShader(player),
    );
    canvas.drawCircle(
      player.center + Offset(u * 12, -u * 6),
      u * 16,
      Paint()
        ..shader =
            RadialGradient(
              colors: [
                const Color(0xFFFFE0A3).withValues(alpha: 0.85),
                const Color(0xFFFFE0A3).withValues(alpha: 0),
              ],
            ).createShader(
              Rect.fromCircle(
                center: player.center + Offset(u * 12, -u * 6),
                radius: u * 16,
              ),
            ),
    );
    // Silhouettes on the stage.
    final ink = Paint()..color = const Color(0xE6100A14);
    for (final (x, hgt) in [
      (0.18, 0.42),
      (0.36, 0.5),
      (0.58, 0.46),
      (0.78, 0.4),
    ]) {
      final cx = player.left + player.width * x;
      final base = player.bottom - u * 3;
      canvas.drawCircle(Offset(cx, base - player.height * hgt), u * 1.6, ink);
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromLTRB(
            cx - u * 2.4,
            base - player.height * hgt + u * 1.8,
            cx + u * 2.4,
            base,
          ),
          Radius.circular(u * 1.5),
        ),
        ink,
      );
    }
    _box(
      canvas,
      Rect.fromLTWH(player.left, player.bottom - u * 3, player.width, u * 3),
      const Color(0x99000000),
    );
    _box(
      canvas,
      Rect.fromLTWH(
        player.left,
        player.bottom - u * 0.5,
        player.width,
        u * 0.5,
      ),
      const Color(0x66FFFFFF),
    );
    _box(
      canvas,
      Rect.fromLTWH(
        player.left,
        player.bottom - u * 0.5,
        player.width * 0.38,
        u * 0.5,
      ),
      const Color(0xFFFF0033),
    );
    canvas.drawCircle(
      Offset(player.left + player.width * 0.38, player.bottom - u * 0.25),
      u * 0.7,
      Paint()..color = const Color(0xFFFF0033),
    );
    _text(
      canvas,
      '1:22 / 3:24',
      Offset(player.left + u * 1.5, player.bottom - u * 2.4),
      u * 1.15,
      Colors.white,
    );
    _text(
      canvas,
      'Imagine Dragons - Believer (Official Music Video)',
      Offset(player.left, player.bottom + u * 1.6),
      u * 2,
      Colors.white,
      weight: FontWeight.w600,
      maxWidth: player.width,
    );
    _text(
      canvas,
      'ImagineDragonsVEVO · 2.6B views',
      Offset(player.left, player.bottom + u * 4.6),
      u * 1.3,
      const Color(0xFFAAAAAA),
    );

    // Up next.
    for (var i = 0; i < 5; i++) {
      final y = top + i * u * 8.6;
      final thumb = Rect.fromLTWH(win.left + u * 70, y, u * 12, u * 6.8);
      canvas.drawRRect(
        RRect.fromRectAndRadius(thumb, Radius.circular(u * 0.8)),
        Paint()
          ..shader = LinearGradient(
            colors: [
              HSVColor.fromAHSV(
                1,
                (i * 57 + 200) % 360.0,
                0.55,
                0.55,
              ).toColor(),
              HSVColor.fromAHSV(1, (i * 57 + 240) % 360.0, 0.65, 0.3).toColor(),
            ],
          ).createShader(thumb),
      );
      _box(
        canvas,
        Rect.fromLTWH(win.left + u * 83.5, y + u * 0.6, u * 12, u * 1),
        const Color(0xFF3F3F3F),
        u * 0.5,
      );
      _box(
        canvas,
        Rect.fromLTWH(win.left + u * 83.5, y + u * 2.4, u * 8, u * 1),
        const Color(0xFF2E2E2E),
        u * 0.5,
      );
    }
    canvas.restore();

    // Taskbar.
    final bar = Rect.fromLTWH(0, h * 0.93, w, h * 0.07);
    _box(canvas, bar, const Color(0xE61C1F2B));
    final icons = [
      const Color(0xFF4CC2FF),
      const Color(0xFFF4B400),
      const Color(0xFF34A853),
      const Color(0xFFEA4335),
      const Color(0xFF2B579A),
      const Color(0xFF0078D4),
    ];
    final s = bar.height * 0.5;
    final start = w / 2 - (icons.length * s * 1.6) / 2;
    for (final (i, color) in icons.indexed) {
      _box(
        canvas,
        Rect.fromLTWH(start + i * s * 1.6, bar.top + bar.height * 0.25, s, s),
        color,
        s * 0.25,
      );
    }
    _text(
      canvas,
      '9:41',
      Offset(w - bar.height * 2.2, bar.top + bar.height * 0.28),
      bar.height * 0.36,
      Colors.white,
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
    find.descendant(of: find.byType(TalvioNavBar), matching: find.text(label)),
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

// What the assistant does today: it acts, then says what it did. There is
// no "Go ahead?" step any more (the desktop sends the email and reports it).
const chat = '''
{"conversation":"c1","conversationComputer":"pc","turns":[
{"computerId":"pc","computerName":"ISHAN-PC","spoken":true,
 "question":"Play Believer by Imagine Dragons on YouTube",
 "answer":"Playing **Believer** by Imagine Dragons on YouTube. Open the live view to watch it here.",
 "actions":[{"label":"Played Believer on YouTube","status":"done"}]},
{"computerId":"pc","computerName":"ISHAN-PC","spoken":false,
 "question":"Email Ron that the meeting moves to 9",
 "answer":"Sent. I wrote to Ron:\\n\\n> Hi Ron, the meeting moves to **9:00** tomorrow. See you then.",
 "actions":[{"label":"Sent to Ron","status":"done"}]}
]}''';

int _at(int day, int hour, int minute) =>
    DateTime(2026, 10, day, hour, minute).millisecondsSinceEpoch;

final commandsRecent =
    '[{"said":"Email me the project report","feedback":"Sent Project report.pdf to you.","status":"done","at":${_at(26, 9, 41)}},'
    '{"said":"Play Believer by Imagine Dragons","feedback":"Playing Believer on YouTube.","status":"done","at":${_at(26, 9, 30)}},'
    '{"said":"Turn the volume down","feedback":"Volume set to 30%.","status":"done","at":${_at(26, 9, 28)}},'
    '{"said":"Open Gmail","feedback":"Opened Gmail in Chrome.","status":"done","at":${_at(25, 18, 2)}}]';

final dictationsRecent =
    '[{"text":"Thanks for the notes. I will send the revised deck by Friday afternoon.","at":${_at(26, 9, 30)}},'
    '{"text":"Can we move the stand-up to ten tomorrow?","at":${_at(25, 17, 5)}}]';

const _windows = [
  {'id': 'w1', 'title': 'Q4 plan.docx', 'app': 'Word', 'front': true},
  {'id': 'w2', 'title': 'Believer - YouTube', 'app': 'Google Chrome'},
  {'id': 'w3', 'title': 'Inbox (3)', 'app': 'Outlook'},
];

const _screens = [
  {
    'id': 's1',
    'name': 'Display 1',
    'width': 2560,
    'height': 1440,
    'primary': true,
  },
];

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
      await shot(tester, '03-ask-chat-$m');
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
      await shot(tester, '26-ask-working-$m');
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
      await tester.enterText(find.byType(TextField), 'Email Ron the Q4 plan');
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
        'transcript': 'Email Ron the Q4 plan',
        'status': 'done',
        'feedback': 'Sent Q4 plan.docx to Ron.',
      });
      await tester.pump();
      await tester.pump();
      await shot(tester, '08-do-done-$m');
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
      await tester.tap(find.byTooltip('Keys'));
      await tester.pump();
      await shot(tester, '25-dictate-keys-$m');
      await tester.tap(find.byTooltip('Hide keys'));
      await tester.pump();
      await tester.tap(find.text('Where the cursor is'));
      await tester.pump();
      reply(server, lastId(server, 'windows.list'), {
        'kind': 'result',
        'ok': true,
        'windows': _windows,
        'screens': _screens,
      });
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await shot(tester, '19-dictate-sheet-$m');
      await tester.tap(find.text('Word').last);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await tester.tap(find.bySemanticsLabel('Start dictating'));
      // The recording's clock is a real stopwatch: let real time pass.
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(seconds: 4)),
      );
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
            'title': 'Claude Code — talvio',
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
      await tester.tap(find.text('Display 1'));
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
      await tester.tap(find.byTooltip('More options'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await shot(tester, '20-live-options-$m');
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

    testWidgets('computers $m', (tester) async {
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
        prefs: {'account.dictate.computer': 'pc'},
      );
      await tester.tap(find.bySemanticsLabel('Choose computer'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await shot(tester, '21-computers-$m');
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

  testWidgets('large text', (tester) async {
    final (_, relay) = await app(
      tester,
      prefs: {
        'account.command.recent': commandsRecent,
        'account.dictate.recent': dictationsRecent,
      },
    );
    tester.view.physicalSize = const Size(360, 780);
    tester.platformDispatcher.textScaleFactorTestValue = 1.3;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    await tester.pump();
    await shot(tester, '22-large-ask');
    await tab(tester, 'Do');
    await shot(tester, '23-large-do');
    await tab(tester, 'Dictate');
    await shot(tester, '24-large-dictate');
    await finish(tester, relay);
  });
}
