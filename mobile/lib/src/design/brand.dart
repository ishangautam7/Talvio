// The Talvio mark and wordmark, drawn rather than shipped as images so they
// stay sharp at every size and need no asset.

import 'package:flutter/material.dart';

import 'tokens.dart';

/// The Wave-T: five waveform bars whose centre bar grows into the stem of a
/// T, white on the coral tile (`src/components/Logo.tsx`, on its 1024 grid).
class TalvioMark extends StatelessWidget {
  const TalvioMark({super.key, this.size = 32, this.tile = true});

  final double size;

  /// Draw the coral tile behind the bars; without it the bars take the
  /// current icon colour.
  final bool tile;

  @override
  Widget build(BuildContext context) {
    return ExcludeSemantics(
      child: CustomPaint(
        size: Size.square(size),
        painter: _MarkPainter(
          tile: tile,
          bars: tile
              ? Colors.white
              : IconTheme.of(context).color ?? Colors.white,
        ),
      ),
    );
  }
}

class _MarkPainter extends CustomPainter {
  const _MarkPainter({required this.tile, required this.bars});

  final bool tile;
  final Color bars;

  static const _bars = [
    (234.0, 340.0, 128.0),
    (352.0, 286.0, 236.0),
    (470.0, 236.0, 564.0),
    (588.0, 286.0, 236.0),
    (706.0, 340.0, 128.0),
  ];

  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / 1024;
    canvas.scale(s);
    if (tile) {
      const rect = Rect.fromLTWH(32, 32, 960, 960);
      canvas.drawRRect(
        RRect.fromRectAndRadius(rect, const Radius.circular(224)),
        Paint()..shader = TalvioColors.gradient.createShader(rect),
      );
    }
    final paint = Paint()..color = bars;
    for (final (x, y, h) in _bars) {
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromLTWH(x, y, 84, h),
          const Radius.circular(42),
        ),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(_MarkPainter old) => old.tile != tile || old.bars != bars;
}

/// "Talvio" as the logo sets it: Plus Jakarta Sans 800, tight.
class Wordmark extends StatelessWidget {
  const Wordmark({super.key, this.size = 22, this.color});

  final double size;
  final Color? color;

  @override
  Widget build(BuildContext context) => Text(
    'Talvio',
    style: TextStyle(
      fontFamily: TalvioFonts.display,
      fontWeight: FontWeight.w800,
      fontSize: size,
      height: 1,
      letterSpacing: -0.02 * size,
      color: color ?? context.talvio.ink,
    ),
  );
}

/// The mark beside the wordmark; its cap height is 60% of the mark's.
class LogoLockup extends StatelessWidget {
  const LogoLockup({super.key, this.size = 28});

  final double size;

  @override
  Widget build(BuildContext context) => Semantics(
    label: 'Talvio',
    excludeSemantics: true,
    child: Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        TalvioMark(size: size),
        SizedBox(width: size * 0.3),
        Wordmark(size: size * 0.82),
      ],
    ),
  );
}
