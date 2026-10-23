import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/strings.dart';
import 'package:talvio_mobile/src/features/screen/view_geometry.dart';

void main() {
  // A phone in portrait watching a 16:9 picture: bars above and below.
  const viewport = Size(400, 800);
  const picture = Size(1920, 1080);

  group('letterbox', () {
    test('the picture is letterboxed to fit', () {
      expect(
        containRect(picture, viewport),
        const Rect.fromLTWH(0, 287.5, 400, 225),
      );
      expect(
        containRect(const Size(1080, 1920), const Size(800, 400)),
        const Rect.fromLTWH(287.5, 0, 225, 400),
      );
    });
  });

  group('follow', () {
    test('zooms until the caret is 4% of the viewport, within range', () {
      // A caret 4% of the picture's height is 9 px here; 32 px wanted.
      expect(
        followScale(
          caret: const Rect.fromLTWH(0.5, 0.5, 0, 0.04),
          viewport: viewport,
          picture: picture,
        ),
        closeTo(32 / 9, 1e-9),
      );
      expect(
        followScale(
          caret: const Rect.fromLTWH(0.5, 0.5, 0, 0.001),
          viewport: viewport,
          picture: picture,
        ),
        6,
        reason: 'capped',
      );
      expect(
        followScale(
          caret: const Rect.fromLTWH(0.5, 0.5, 0, 0.5),
          viewport: viewport,
          picture: picture,
        ),
        1,
        reason: 'never below the whole picture',
      );
    });

    test('centres the caret, without showing past the edges', () {
      const caret = Rect.fromLTWH(0.5, 0.5, 0, 0.04);
      final view = followTransform(
        current: Matrix4.identity(),
        caret: caret,
        viewport: viewport,
        picture: picture,
      );
      final scale = view.getMaxScaleOnAxis();
      expect(scale, closeTo(32 / 9, 1e-9));
      final shown = MatrixUtils.transformRect(
        view,
        const Rect.fromLTWH(200, 400, 0, 9),
      );
      expect(shown.center.dx, closeTo(200, 1e-6));
      expect(shown.center.dy, closeTo(400, 1e-6));

      // A caret in a corner: the view stops at the picture's corner.
      final corner = followTransform(
        current: Matrix4.identity(),
        caret: const Rect.fromLTWH(0, 0, 0, 0.04),
        viewport: viewport,
        picture: picture,
      );
      final t = corner.getTranslation();
      expect(t.x, 0);
      expect(t.y, lessThanOrEqualTo(0));
      expect(t.y, greaterThanOrEqualTo(800 - 800 * corner.getMaxScaleOnAxis()));
    });

    test('stays put while the caret moves inside the view', () {
      const caret = Rect.fromLTWH(0.5, 0.5, 0, 0.04);
      final view = followTransform(
        current: Matrix4.identity(),
        caret: caret,
        viewport: viewport,
        picture: picture,
      );
      final typed = followTransform(
        current: view,
        caret: caret.translate(0.01, 0),
        viewport: viewport,
        picture: picture,
      );
      expect(identical(typed, view), isTrue);
      final far = followTransform(
        current: view,
        caret: caret.translate(0.4, 0.3),
        viewport: viewport,
        picture: picture,
      );
      expect(far, isNot(view));
    });
  });

  group('protocol', () {
    test('screens are read from windows.list, bad entries dropped', () {
      final screens = screensOf({
        'screens': [
          {
            'id': 's3',
            'name': 'Display 1',
            'width': 2560,
            'height': 1440,
            'primary': true,
          },
          {'id': 's4', 'name': 'Display 2', 'width': 1920, 'height': 1080},
          {'id': '', 'name': 'x', 'width': 1, 'height': 1},
          {'id': 's5', 'name': 'x', 'width': 0, 'height': 1},
          'nope',
        ],
      });
      expect(screens.map((s) => s.id), ['s3', 's4']);
      expect(screens.first.primary, isTrue);
      expect(screens.last.primary, isFalse);
      expect(screensOf({'windows': []}), isEmpty);
    });

    test('control messages', () {
      expect(ControlMessage.decode('not json'), isNull);
      expect(ControlMessage.decode('{"t":"other"}'), isNull);
      expect(
        ControlMessage.decode('{"t":"ack","id":"k3","ok":true}'),
        isNull,
        reason: 'this phone sends no input to acknowledge',
      );
      expect(
        (ControlMessage.decode('{"t":"caret"}')! as CaretMessage).caret,
        isNull,
      );
    });
  });

  test('error codes read as short messages', () {
    String text(String code) => ScreenStrings.error(code, 'ISHAN-PC');
    expect(text('stopped'), 'Stopped on the computer.');
    expect(text('connect_failed'), contains('TURN server'));
    expect(text('busy'), 'Someone is already watching this computer.');
    for (final code in [
      'target_gone',
      'idle',
      'capture_failed',
      'remote_windows_off',
      'remote_input_off',
      'not_approved',
    ]) {
      expect(text(code), isNot(contains('Something went wrong')), reason: code);
    }
    expect(text('brand_new_code'), 'Something went wrong. Try again.');
  });

  group('fill', () {
    test('a wide picture on a tall view fills its height, centred', () {
      const picture = Size(1920, 1080);
      const viewport = Size(400, 700);
      final scale = fillScale(picture, viewport);
      expect(scale, moreOrLessEquals(700 / 225));
      final fill = fillTransform(picture, viewport);
      final shown = MatrixUtils.transformRect(
        fill,
        containRect(picture, viewport),
      );
      expect(shown.top, moreOrLessEquals(0));
      expect(shown.bottom, moreOrLessEquals(700));
      expect(shown.center.dx, moreOrLessEquals(200), reason: 'centred');
      expect(shown.center.dy, moreOrLessEquals(350), reason: 'centred');
    });

    test('a picture that already fills the view stays as it is', () {
      expect(fillScale(const Size(400, 700), const Size(400, 700)), 1);
      expect(fillScale(Size.zero, const Size(400, 700)), 1);
    });
  });
}
