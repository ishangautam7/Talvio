// The viewer's geometry, kept pure so it is tested without a picture.
//
// The viewer is an InteractiveViewer whose child fills the viewport; the
// video is letterboxed (contain) inside that child; and the transform maps
// child coordinates to viewport coordinates (a uniform scale, then a
// translation). The computer speaks in 0..1 of the picture.

import 'dart:math' as math;

import 'package:flutter/widgets.dart';

/// The rect a picture of size [picture] takes inside [viewport] when it is
/// fitted whole and centred, with bars on the sides that do not fill.
Rect containRect(Size picture, Size viewport) {
  if (picture.isEmpty || viewport.isEmpty) return Offset.zero & viewport;
  final scale = math.min(
    viewport.width / picture.width,
    viewport.height / picture.height,
  );
  final size = picture * scale;
  return Rect.fromLTWH(
    (viewport.width - size.width) / 2,
    (viewport.height - size.height) / 2,
    size.width,
    size.height,
  );
}

/// The point of the picture under [point] (in viewport coordinates), in 0..1
/// across the picture; null when it falls on a letterbox bar or [transform]
/// cannot be inverted.
Offset? pictureCoords({
  required Offset point,
  required Matrix4 transform,
  required Size viewport,
  required Size picture,
}) {
  final inverse = Matrix4.tryInvert(transform);
  if (inverse == null) return null;
  final child = MatrixUtils.transformPoint(inverse, point);
  final rect = containRect(picture, viewport);
  if (rect.isEmpty) return null;
  final x = (child.dx - rect.left) / rect.width;
  final y = (child.dy - rect.top) / rect.height;
  if (x < 0 || x > 1 || y < 0 || y > 1) return null;
  return Offset(x, y);
}

/// A uniform scale [scale] followed by a translation by [translation].
Matrix4 scaleThenTranslate(double scale, Offset translation) => Matrix4(
  scale,
  0,
  0,
  0, //
  0,
  scale,
  0,
  0,
  0,
  0,
  1,
  0,
  translation.dx,
  translation.dy,
  0,
  1,
);

/// Follow's settings: the caret's height as a share of the viewport's at the
/// zoom it picks, the zoom range, and the margin (a share of each side) the
/// caret may move in before the view moves after it.
class FollowRule {
  const FollowRule({
    this.caretShare = 0.04,
    this.minScale = 1,
    this.maxScale = 6,
    this.margin = 0.15,
  });

  final double caretShare;
  final double minScale;
  final double maxScale;
  final double margin;
}

/// The zoom at which [caret] (0..1 of the picture) is [FollowRule.caretShare]
/// of the viewport's height, within the rule's range.
double followScale({
  required Rect caret,
  required Size viewport,
  required Size picture,
  FollowRule rule = const FollowRule(),
}) {
  final rect = containRect(picture, viewport);
  final height = caret.height * rect.height;
  if (height <= 0) return rule.minScale;
  return (rule.caretShare * viewport.height / height).clamp(
    rule.minScale,
    rule.maxScale,
  );
}

/// The transform that shows [caret] readably: [current] itself while the
/// caret is already in view (inside the margin) at about the right zoom, so
/// typing along a line does not slide the picture on every letter; else the
/// caret centred at [followScale], kept from showing past the picture's edge.
Matrix4 followTransform({
  required Matrix4 current,
  required Rect caret,
  required Size viewport,
  required Size picture,
  FollowRule rule = const FollowRule(),
}) {
  if (viewport.isEmpty) return current;
  final rect = containRect(picture, viewport);
  final scale = followScale(
    caret: caret,
    viewport: viewport,
    picture: picture,
    rule: rule,
  );
  final inChild = Rect.fromLTWH(
    rect.left + caret.left * rect.width,
    rect.top + caret.top * rect.height,
    caret.width * rect.width,
    caret.height * rect.height,
  );

  final currentScale = current.getMaxScaleOnAxis();
  if ((currentScale - scale).abs() <= scale * 0.1) {
    final shown = MatrixUtils.transformRect(current, inChild);
    final safe = Rect.fromLTRB(
      viewport.width * rule.margin,
      viewport.height * rule.margin,
      viewport.width * (1 - rule.margin),
      viewport.height * (1 - rule.margin),
    );
    if (safe.contains(shown.topLeft) && safe.contains(shown.bottomLeft)) {
      return current;
    }
  }

  // A wide caret rect (a selection) is centred on its start, not its middle.
  final focusX = math.min(
    inChild.center.dx,
    inChild.left + viewport.width * 0.4 / scale,
  );
  final focus = Offset(focusX, inChild.center.dy);
  final tx = (viewport.width / 2 - scale * focus.dx).clamp(
    viewport.width - scale * viewport.width,
    0.0,
  );
  final ty = (viewport.height / 2 - scale * focus.dy).clamp(
    viewport.height - scale * viewport.height,
    0.0,
  );
  return scaleThenTranslate(scale, Offset(tx, ty));
}

/// Wheel notches for a two-finger drag of [dy] logical pixels: one per
/// [perNotch], dragging up (negative) scrolling down (positive), as a finger
/// moves a page on a touch screen.
int scrollNotches(double dy, {double perNotch = 40}) =>
    (-dy / perNotch).truncate();

/// The zoom at which [picture] fills [viewport] with no bars, cropping the
/// sides that do not fit (1 when it already fills it).
double fillScale(Size picture, Size viewport) {
  final rect = containRect(picture, viewport);
  if (rect.isEmpty) return 1;
  return math.max(viewport.width / rect.width, viewport.height / rect.height);
}

/// The transform that fills [viewport] with the middle of [picture]; the
/// rest is a pan away.
Matrix4 fillTransform(Size picture, Size viewport) {
  final scale = fillScale(picture, viewport);
  return scaleThenTranslate(
    scale,
    Offset(viewport.width * (1 - scale) / 2, viewport.height * (1 - scale) / 2),
  );
}
