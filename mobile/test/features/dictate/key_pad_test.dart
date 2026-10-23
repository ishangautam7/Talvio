import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/dictate/key_pad.dart';

void main() {
  /// The pad alone, [width] wide, recording what it presses.
  Future<List<List<String>>> pumpPad(
    WidgetTester tester, {
    double width = 360,
    String platform = 'windows',
    bool enabled = true,
  }) async {
    final pressed = <List<String>>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: SizedBox(
              width: width,
              child: KeyPad(
                platform: platform,
                onPress: enabled ? pressed.add : null,
              ),
            ),
          ),
        ),
      ),
    );
    return pressed;
  }

  Rect rectOf(WidgetTester tester, String label) =>
      tester.getRect(find.byTooltip(label));

  testWidgets('the arrows are an inverted T, Up over Down', (tester) async {
    await pumpPad(tester);
    final up = rectOf(tester, 'Up arrow');
    final left = rectOf(tester, 'Left arrow');
    final down = rectOf(tester, 'Down arrow');
    final right = rectOf(tester, 'Right arrow');

    expect(up.center.dx, moreOrLessEquals(down.center.dx));
    expect(up.bottom, lessThan(down.top), reason: 'Up sits above');
    for (final key in [left, right]) {
      expect(key.center.dy, moreOrLessEquals(down.center.dy));
    }
    expect(left.right, lessThan(down.left));
    expect(down.right, lessThan(right.left));
    // Nothing on the T's left shoulder.
    expect(
      find.byWidgetPredicate(
        (w) =>
            w is Tooltip &&
            tester.getRect(find.byWidget(w)).center.dx < up.left &&
            tester.getRect(find.byWidget(w)).center.dx > left.left &&
            tester.getRect(find.byWidget(w)).center.dy < up.bottom,
      ),
      findsNothing,
    );
    // The arrows are right of every editing key.
    for (final label in ['Escape', 'Tab', 'Backspace', 'Space', 'Enter']) {
      expect(rectOf(tester, label).right, lessThan(left.left), reason: label);
    }
  });

  testWidgets('keys are big enough to hit, and Enter is the big one', (
    tester,
  ) async {
    await pumpPad(tester);
    for (final label in [
      'Escape',
      'Tab',
      'Backspace',
      'Space',
      'Enter',
      'Up arrow',
      'Left arrow',
      'Down arrow',
      'Right arrow',
    ]) {
      final rect = rectOf(tester, label);
      expect(rect.height, greaterThanOrEqualTo(48), reason: label);
      expect(rect.width, greaterThanOrEqualTo(48), reason: label);
    }
    expect(
      rectOf(tester, 'Enter').width,
      greaterThan(rectOf(tester, 'Tab').width * 1.8),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('every key presses its combo; More has the shortcuts', (
    tester,
  ) async {
    final handle = tester.ensureSemantics();
    final pressed = await pumpPad(tester, platform: 'macos');
    for (final label in [
      'Escape',
      'Tab',
      'Backspace',
      'Space',
      'Enter',
      'Up arrow',
      'Left arrow',
      'Down arrow',
      'Right arrow',
    ]) {
      await tester.tap(find.bySemanticsLabel(label));
      await tester.pump();
    }
    expect(pressed, [
      ['escape'],
      ['tab'],
      ['backspace'],
      ['space'],
      ['enter'],
      ['up'],
      ['left'],
      ['down'],
      ['right'],
    ]);

    await tester.tap(find.byTooltip('More keys'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    for (final item in [
      'Shift + Enter (new line)',
      'Cmd + V (paste)',
      'Cmd + Z (undo)',
      'Cmd + A (select all)',
    ]) {
      expect(find.text(item), findsOneWidget);
    }
    await tester.tap(find.text('Cmd + C (copy)'));
    await tester.pump(const Duration(milliseconds: 500));
    expect(pressed.last, ['cmd+c']);
    handle.dispose();
  });

  testWidgets('screen readers hear each key by name', (tester) async {
    final handle = tester.ensureSemantics();
    await pumpPad(tester, enabled: false);
    expect(
      tester.getSemantics(find.bySemanticsLabel('Up arrow')),
      isSemantics(isButton: true, isEnabled: false),
    );
    handle.dispose();
  });

  testWidgets('fits the narrowest phone and a wide tablet', (tester) async {
    for (final width in [288.0, 600.0]) {
      await pumpPad(tester, width: width);
      expect(tester.takeException(), isNull, reason: '$width');
      final pad = tester.getRect(find.byType(KeyPad));
      final right = rectOf(tester, 'Right arrow');
      expect(right.right, lessThanOrEqualTo(pad.right + 0.5));
    }
  });
}
