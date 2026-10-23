import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:zynveliq_mobile/src/features/dictate/dictate_controller.dart';
import 'package:zynveliq_mobile/src/features/dictate/dictate_screen.dart';
import 'package:zynveliq_mobile/src/features/dictate/recent_dictations.dart';

import '../../helpers.dart';

void main() {
  test('computersOf keeps computers only, online first', () {
    final list = computersOf([
      device(id: 'phone', kind: 'mobile'),
      device(id: 'a', online: false),
      device(id: 'b'),
      device(id: 'c', online: false),
      device(id: 'd'),
    ]);
    expect(list.map((d) => d.id), ['b', 'd', 'a', 'c']);
  });

  group('pickComputer', () {
    final a = device(id: 'a');
    final b = device(id: 'b');
    final offline = device(id: 'off', online: false);

    test("the account's only computer, online or not, without a choice", () {
      expect(pickComputer([a], null), a);
      expect(pickComputer([offline], null), offline);
    });

    test('the remembered choice, online or not', () {
      expect(pickComputer([a, b], 'b'), b);
      expect(pickComputer([a, offline], 'off'), offline);
      expect(pickComputer([offline], 'off'), offline);
    });

    test('nothing with several computers and no choice, or a gone one', () {
      expect(pickComputer([a, b], null), isNull);
      expect(pickComputer([a, offline], null), isNull, reason: 'one online');
      expect(pickComputer([a, b], 'gone'), isNull);
      expect(pickComputer([], 'a'), isNull);
      expect(pickComputer([b], 'gone'), isNull, reason: 'one left, unchosen');
    });

    test('a computer that joins later is never chosen on its own', () {
      final late = device(id: 'late');
      expect(pickComputer([a, late], null), isNull);
      expect(pickComputer([a, late], 'a'), a);
    });
  });

  test('recent dictations keep the newest 20 until sign-out', () async {
    final session = await sessionStore();
    final prefs = await SharedPreferences.getInstance();
    final recents = RecentDictations(prefs);
    for (var i = 0; i < 25; i++) {
      await recents.add('dictation $i');
    }
    expect(recents.items, hasLength(RecentDictations.limit));
    expect(recents.items.first.text, 'dictation 24');
    expect(RecentDictations(prefs).items.map((r) => r.text), [
      for (var i = 24; i >= 5; i--) 'dictation $i',
    ]);

    await session.clear();
    expect(RecentDictations(prefs).items, isEmpty);
  });

  test('typed text is capped at 10 000 code points, not graphemes', () {
    // Thumbs up with a skin tone: one grapheme, two code points.
    final thumbs = '\u{1F44D}\u{1F3FD}' * 6000;
    TextEditingValue type(String text) => typedTextLimit.formatEditUpdate(
      TextEditingValue.empty,
      TextEditingValue(text: text),
    );
    expect(type(thumbs).text.runes.length, 10000);
    expect(type('x' * 10000).text, 'x' * 10000);
  });
}
