import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/screen/live_protocol.dart';
import 'package:talvio_mobile/src/features/screen/strings.dart';
import 'package:talvio_mobile/src/features/screen/view_geometry.dart';

void main() {
  // A phone in portrait watching a 16:9 picture: bars above and below.
  const viewport = Size(400, 800);
  const picture = Size(1920, 1080);

  group('tap mapping', () {
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

    test('unzoomed: across the picture, null on the bars', () {
      Offset? at(Offset p) => pictureCoords(
        point: p,
        transform: Matrix4.identity(),
        viewport: viewport,
        picture: picture,
      );
      expect(at(const Offset(200, 400)), const Offset(0.5, 0.5));
      expect(at(const Offset(0, 287.5)), Offset.zero);
      expect(at(const Offset(400, 512.5)), const Offset(1, 1));
      expect(at(const Offset(100, 343.75)), const Offset(0.25, 0.25));
      expect(at(const Offset(200, 100)), isNull, reason: 'top bar');
      expect(at(const Offset(200, 700)), isNull, reason: 'bottom bar');
    });

    test('zoomed and panned: through the inverse of the view', () {
      // 2x, moved so child point (100, 287.5) (the picture's top-left
      // quarter point) sits at the viewport's origin.
      final view = scaleThenTranslate(2, const Offset(-200, -575));
      Offset? at(Offset p) => pictureCoords(
        point: p,
        transform: view,
        viewport: viewport,
        picture: picture,
      );
      expect(at(Offset.zero), const Offset(0.25, 0));
      expect(at(const Offset(200, 225)), const Offset(0.5, 0.5));
      expect(at(const Offset(0, -10)), isNull, reason: 'above the picture');
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

    test('two fingers dragging up scroll down, one notch per 40 px', () {
      expect(scrollNotches(-85), 2);
      expect(scrollNotches(30), 0);
      expect(scrollNotches(45), -1);
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
      final ack = ControlMessage.decode(
        '{"t":"ack","id":"k3","ok":false,"error":"confirm","label":"Send"}',
      );
      expect(ack, isA<AckMessage>());
      expect((ack! as AckMessage).label, 'Send');
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
      'not_allowed',
      'password_field',
      'key_refused',
      'keys_failed',
      'paste_failed',
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
      // What is shown still maps back to the picture.
      final at = pictureCoords(
        point: const Offset(200, 350),
        transform: fill,
        viewport: viewport,
        picture: picture,
      )!;
      expect(at.dx, moreOrLessEquals(0.5));
      expect(at.dy, moreOrLessEquals(0.5));
    });

    test('a picture that already fills the view stays as it is', () {
      expect(fillScale(const Size(400, 700), const Size(400, 700)), 1);
      expect(fillScale(Size.zero, const Size(400, 700)), 1);
    });
  });
}
