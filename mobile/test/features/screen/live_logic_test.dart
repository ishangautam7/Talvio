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

  group('a touch on the viewer to a point of the picture', () {
    test('takes the letterbox off', () {
      // The picture spans y 287.5..512.5 of the 800 tall viewport.
      Offset? at(Offset point) => pictureCoords(
        point: point,
        transform: Matrix4.identity(),
        viewport: viewport,
        picture: picture,
      );
      expect(at(const Offset(200, 400)), const Offset(0.5, 0.5));
      expect(at(const Offset(0, 287.5)), Offset.zero);
      expect(at(const Offset(400, 512.5)), const Offset(1, 1));
      expect(at(const Offset(200, 100)), isNull, reason: 'the bar above');
      expect(at(const Offset(200, 700)), isNull, reason: 'the bar below');
    });

    test('undoes the zoom and the pan', () {
      // Zoomed 4x and panned.
      final view = scaleThenTranslate(4, const Offset(-200, -1150));
      final middle = pictureCoords(
        point: const Offset(200, 400),
        transform: view,
        viewport: viewport,
        picture: picture,
      )!;
      // Child point: ((200 + 200) / 4, (400 + 1150) / 4) = (100, 387.5).
      expect(middle.dx, closeTo(100 / 400, 1e-9));
      expect(middle.dy, closeTo((387.5 - 287.5) / 225, 1e-9));

      // Every corner of the zoomed view maps back through the same matrix.
      for (final point in const [Offset(0, 0), Offset(400, 800)]) {
        final at = pictureCoords(
          point: point,
          transform: view,
          viewport: viewport,
          picture: picture,
        );
        final child = MatrixUtils.transformPoint(Matrix4.inverted(view), point);
        expect(at!.dx, closeTo(child.dx / 400, 1e-9));
        expect(at.dy, closeTo((child.dy - 287.5) / 225, 1e-9));
      }
    });

    test('a transform that cannot be inverted maps nothing', () {
      expect(
        pictureCoords(
          point: const Offset(200, 400),
          transform: Matrix4.zero(),
          viewport: viewport,
          picture: picture,
        ),
        isNull,
      );
    });

    test('two fingers dragged up scroll down, one notch per 40 points', () {
      expect(scrollNotches(-40), 1);
      expect(scrollNotches(-119), 2);
      expect(scrollNotches(39), 0);
      expect(scrollNotches(85), -2);
      expect(scrollNotches(-60, perNotch: 20), 3);
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
      final ack =
          ControlMessage.decode(
                '{"t":"ack","id":"k3","ok":false,"error":"key_refused"}',
              )!
              as AckMessage;
      expect((ack.id, ack.ok, ack.error), ('k3', false, 'key_refused'));
      expect(
        ControlMessage.decode('{"t":"ack","ok":true}'),
        isNull,
        reason: 'an ack without its id',
      );
      expect(
        (ControlMessage.decode('{"t":"caret"}')! as CaretMessage).caret,
        isNull,
      );
      final confirm =
          ControlMessage.decode(
                '{"t":"ack","id":"k4","ok":false,"error":"confirm",'
                '"label":" Send "}',
              )!
              as AckMessage;
      expect((confirm.error, confirm.label), ('confirm', 'Send'));
    });

    test('input messages read as the protocol says', () {
      expect(ControlInput.keys(['enter']), {
        't': 'keys',
        'keys': ['enter'],
      });
      expect(ControlInput.text('hi'), {'t': 'text', 'text': 'hi'});
      expect(ControlInput.tap(const Offset(0.42, 0.81)), {
        't': 'tap',
        'x': 0.42,
        'y': 0.81,
        'button': 'left',
        'count': 1,
        'confirmed': false,
      });
      expect(
        ControlInput.tap(
          const Offset(0.123456789, 1.2),
          right: true,
          count: 2,
          confirmed: true,
        ),
        {
          't': 'tap',
          'x': 0.1235,
          'y': 1.0,
          'button': 'right',
          'count': 2,
          'confirmed': true,
        },
        reason: 'rounded, and kept inside the picture',
      );
      expect(ControlInput.tap(Offset.zero, count: 3)['count'], 1);
      expect(ControlInput.scroll(const Offset(0.5, -0.1), dy: 3), {
        't': 'scroll',
        'x': 0.5,
        'y': 0.0,
        'dy': 3,
        'dx': 0,
      });
    });

    test('an input result knows a confirm', () {
      const confirm = InputResult.failed('confirm', label: 'Send');
      expect((confirm.ok, confirm.needsConfirm), (false, true));
      expect(const InputResult.ok().ok, isTrue);
    });
  });

  test('error codes read as short messages', () {
    String text(String code) => ScreenStrings.error(code, 'ISHAN-PC');
    expect(text('stopped'), 'Stopped on the computer.');
    expect(text('connect_failed'), contains('TURN server'));
    expect(
      text('busy'),
      'Another view of ISHAN-PC is still running. Try again in a moment.',
    );
    expect(text('replaced'), contains('replaced by a newer one'));
    for (final code in [
      'target_gone',
      'idle',
      'capture_failed',
      'remote_windows_off',
      'remote_input_off',
      'not_approved',
      'not_allowed',
      'password_field',
      'tap_failed',
      'not_live',
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
