// The Talvio voice orb: a glass sphere of coral and orange light drifting
// over the ink tile's indigo, with a soft halo. The same idea as the
// desktop's LuminousOrb (src/assistant/orbShader.ts), painted with
// gradients instead of a shader so it costs next to nothing on a phone.
//
// It is the one piece of the app allowed to glow: the Ask greeting, the
// Dictate mic, and the small "working" mark beside a request in flight.

import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'components.dart' show loopsAllowed;
import 'tokens.dart';

/// What the orb is doing, which sets how it moves.
enum OrbMood {
  /// At rest: a slow drift and a faint breath.
  idle,

  /// Hearing the user: swells with [TalvioOrb.level].
  listening,

  /// Working on something: the light turns faster.
  thinking,
}

class TalvioOrb extends StatefulWidget {
  const TalvioOrb({
    super.key,
    this.size = 96,
    this.mood = OrbMood.idle,
    this.level,
    this.halo = true,
  });

  /// The sphere's diameter. The halo spills past it and takes no layout.
  final double size;
  final OrbMood mood;

  /// The input level (0–1) while [OrbMood.listening].
  final ValueListenable<double>? level;

  /// Paint the soft light around the sphere.
  final bool halo;

  @override
  State<TalvioOrb> createState() => _TalvioOrbState();
}

class _TalvioOrbState extends State<TalvioOrb>
    with SingleTickerProviderStateMixin {
  late final _clock = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 1),
  )..addListener(_tick);

  /// The orb's own time in seconds, advanced faster while thinking.
  var _time = 1.7;
  var _last = Duration.zero;
  var _energy = 0.0;
  var _moving = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(TalvioOrb old) {
    super.didUpdateWidget(old);
    _sync();
  }

  void _sync() {
    final moving = loopsAllowed(context);
    if (moving == _moving) return;
    _moving = moving;
    if (moving) {
      _last = Duration.zero;
      _clock.repeat();
    } else {
      _clock.stop();
    }
  }

  void _tick() {
    final now = _clock.lastElapsedDuration ?? Duration.zero;
    var dt = (now - _last).inMicroseconds / 1e6;
    if (dt < 0 || dt > 0.25) dt = 1 / 60;
    _last = now;
    final speed = switch (widget.mood) {
      OrbMood.idle => 0.18,
      OrbMood.listening => 0.32,
      OrbMood.thinking => 0.9,
    };
    _time += dt * speed;
    final target = widget.mood == OrbMood.listening
        ? (widget.level?.value ?? 0).clamp(0.0, 1.0)
        : 0.0;
    // Quick to rise, slow to fall, like a level meter.
    _energy += (target - _energy) * (target > _energy ? 0.35 : 0.08);
    setState(() {});
  }

  @override
  void dispose() {
    _clock.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final dark = context.talvio.isDark;
    // Without motion the listening orb still shows the voice, statically.
    final energy = _moving
        ? _energy
        : widget.mood == OrbMood.listening
        ? 0.35
        : 0.0;
    return ExcludeSemantics(
      child: RepaintBoundary(
        child: CustomPaint(
          size: Size.square(widget.size),
          painter: OrbPainter(
            time: _time,
            energy: energy,
            halo: widget.halo,
            dark: dark,
          ),
        ),
      ),
    );
  }
}

/// Paints the orb into its square; the halo reaches past it.
class OrbPainter extends CustomPainter {
  const OrbPainter({
    required this.time,
    this.energy = 0,
    this.halo = true,
    this.dark = false,
  });

  final double time;
  final double energy;
  final bool halo;
  final bool dark;

  static const _peach = Color(0xFFFFD2A3);
  static const _orange = Color(0xFFFF8F3F);
  static const _coral = Color(0xFFF2543D);
  static const _rose = Color(0xFFE23D6B);
  static const _indigo = Color(0xFF3B2A8C);
  static const _night = Color(0xFF1B1446);

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final base = size.shortestSide / 2;
    final breath = math.sin(time * 2 * math.pi * 0.35) * 0.018;
    final r = base * (1 + breath + 0.07 * energy);

    if (halo) {
      // Small orbs (a status mark) get a fainter halo, or it reads as a
      // smudge rather than light.
      final k = (base / 40).clamp(0.3, 1.0);
      // A wide, quiet light (warm on a light page, where indigo turns to
      // grey), then a softer warm one close to the glass.
      final wide = r * (2.2 + 0.5 * energy);
      final soft = ((dark ? 0.26 : 0.2) + 0.12 * energy) * k;
      final far = dark ? _indigo : _orange;
      canvas.drawCircle(
        c,
        wide,
        Paint()
          ..shader = RadialGradient(
            colors: [
              (dark ? _rose : _coral).withValues(alpha: soft),
              far.withValues(alpha: soft * 0.3),
              far.withValues(alpha: 0),
            ],
            stops: const [0.3, 0.62, 1.0],
          ).createShader(Rect.fromCircle(center: c, radius: wide)),
      );
      final near = r * (1.5 + 0.3 * energy);
      final warm = ((dark ? 0.45 : 0.32) + 0.2 * energy) * k;
      canvas.drawCircle(
        c,
        near,
        Paint()
          ..shader = RadialGradient(
            colors: [
              _coral.withValues(alpha: warm),
              _coral.withValues(alpha: warm * 0.4),
              _coral.withValues(alpha: 0),
            ],
            stops: const [0.55, 0.74, 1.0],
          ).createShader(Rect.fromCircle(center: c, radius: near)),
      );
    }

    final sphere = Rect.fromCircle(center: c, radius: r);
    canvas.save();
    canvas.clipPath(Path()..addOval(sphere));

    // The body: warm light from the upper left sinking into indigo.
    canvas.drawRect(
      sphere,
      Paint()
        ..shader = const RadialGradient(
          center: Alignment(-0.3, -0.45),
          radius: 1.15,
          colors: [_peach, _orange, _coral, _rose, _indigo, _night],
          stops: [0.0, 0.22, 0.44, 0.66, 0.88, 1.0],
        ).createShader(sphere),
    );

    // Light from inside, low in the glass.
    final core = Rect.fromCircle(
      center: c + Offset(r * 0.12, r * 0.32),
      radius: r * 0.75,
    );
    canvas.drawRect(
      sphere,
      Paint()
        ..blendMode = BlendMode.screen
        ..shader = RadialGradient(
          colors: [
            const Color(0xFFFF9A6B).withValues(alpha: 0.55 + 0.2 * energy),
            const Color(0xFFFF9A6B).withValues(alpha: 0),
          ],
        ).createShader(core),
    );

    // Ribbons: soft arcs of light curling round the inside, drifting.
    void ribbon({
      required double turn,
      required double offset,
      required double radius,
      required double sweep,
      required double width,
      required Color color,
      required double alpha,
      Offset shift = Offset.zero,
    }) {
      final start = 2 * math.pi * (time * turn + offset);
      final arc = Rect.fromCircle(center: c + shift * r, radius: r * radius);
      canvas.drawArc(
        arc,
        start,
        sweep,
        false,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeCap = StrokeCap.round
          ..strokeWidth = r * width
          ..blendMode = BlendMode.screen
          ..maskFilter = MaskFilter.blur(BlurStyle.normal, r * 0.09)
          ..color = color.withValues(alpha: (alpha + 0.2 * energy).clamp(0, 1)),
      );
    }

    ribbon(
      turn: 0.09,
      offset: 0.62,
      radius: 0.62,
      sweep: 2.1,
      width: 0.16,
      color: const Color(0xFFFFE3C4),
      alpha: 0.7,
      shift: const Offset(-0.08, 0.05),
    );
    ribbon(
      turn: -0.07,
      offset: 0.1,
      radius: 0.48,
      sweep: 1.7,
      width: 0.13,
      color: const Color(0xFFFF5C8A),
      alpha: 0.6,
      shift: const Offset(0.12, 0.1),
    );
    ribbon(
      turn: 0.05,
      offset: 0.33,
      radius: 0.78,
      sweep: 1.3,
      width: 0.1,
      color: const Color(0xFFFFB066),
      alpha: 0.55,
    );

    // Depth: the lower right falls into shadow.
    canvas.drawRect(
      sphere,
      Paint()
        ..shader = RadialGradient(
          center: const Alignment(0.6, 0.8),
          radius: 0.85,
          colors: [_night.withValues(alpha: 0.32), _night.withValues(alpha: 0)],
        ).createShader(sphere),
    );

    // A soft highlight, never a hard specular dot.
    final shine = Rect.fromCenter(
      center: c + Offset(-r * 0.3, -r * 0.5),
      width: r * 0.9,
      height: r * 0.5,
    );
    canvas.drawOval(
      shine,
      Paint()
        ..maskFilter = MaskFilter.blur(BlurStyle.normal, r * 0.06)
        ..shader = RadialGradient(
          colors: [
            Colors.white.withValues(alpha: 0.5),
            Colors.white.withValues(alpha: 0),
          ],
        ).createShader(shine),
    );
    canvas.restore();

    // The glass edge: bright at the top, a warm backlight at the bottom.
    canvas.drawCircle(
      c,
      r - 0.5,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = math.max(1, r * 0.03)
        ..shader = LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            Colors.white.withValues(alpha: 0.6),
            Colors.white.withValues(alpha: 0.0),
            const Color(0xFFFFB27A).withValues(alpha: 0.55),
          ],
          stops: const [0.0, 0.55, 1.0],
        ).createShader(sphere),
    );
  }

  @override
  bool shouldRepaint(OrbPainter old) =>
      old.time != time ||
      old.energy != energy ||
      old.halo != halo ||
      old.dark != dark;
}
