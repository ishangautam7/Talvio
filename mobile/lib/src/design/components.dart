// The building blocks every screen is made of: cards, tiles, notices,
// empty states, pills and the small animations that say "working".

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'tokens.dart';

/// Looping decoration (breathing tiles, rising dots, pulsing live dots).
/// Widget tests turn it off so `pumpAndSettle` settles; spinners that say
/// "working" are separate and always run.
bool ambientMotion = true;

/// Haptics, in one place so every screen uses the same three.
abstract final class Haptic {
  /// A selection: a tab, a chip, a window.
  static void tick() => unawaited(HapticFeedback.selectionClick());

  /// Something starts or is sent: recording, a question.
  static void tap() => unawaited(HapticFeedback.lightImpact());

  /// Something firm: stop recording, Do it.
  static void firm() => unawaited(HapticFeedback.mediumImpact());
}

/// The semantic tone of a notice, a pill or an icon tile.
enum Tone { neutral, accent, success, warning, danger, info }

extension ToneColors on Tone {
  (Color fg, Color bg) colors(TalvioColors t) => switch (this) {
    Tone.neutral => (t.muted, t.disabledFill),
    Tone.accent => (t.accent, t.accentSoft),
    Tone.success => (t.success, t.successSoft),
    Tone.warning => (t.warning, t.warningSoft),
    Tone.danger => (t.danger, t.dangerSoft),
    Tone.info => (t.ink, t.disabledFill),
  };
}

/// A card: the card colour, a hairline and rounded corners. With [onTap] it
/// is a button that dips a little when pressed.
class Surface extends StatelessWidget {
  const Surface({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(Space.lg),
    this.onTap,
    this.color,
    this.borderColor,
    this.radius = Radii.lg,
    this.semanticLabel,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;
  final VoidCallback? onTap;
  final Color? color;
  final Color? borderColor;
  final double radius;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final shape = RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(radius),
      side: BorderSide(color: borderColor ?? t.hairline),
    );
    final body = Padding(padding: padding, child: child);
    final material = Material(
      color: color ?? t.card,
      shape: shape,
      clipBehavior: Clip.antiAlias,
      child: onTap == null ? body : InkWell(onTap: onTap, child: body),
    );
    if (onTap == null) return material;
    return Pressable(onTap: onTap, child: material);
  }
}

/// Shrinks its child a touch while pressed. The tap itself is the child's.
class Pressable extends StatefulWidget {
  const Pressable({super.key, required this.child, this.onTap});

  final Widget child;
  final VoidCallback? onTap;

  @override
  State<Pressable> createState() => _PressableState();
}

class _PressableState extends State<Pressable> {
  var _down = false;

  void _set(bool down) {
    if (widget.onTap != null && down != _down) setState(() => _down = down);
  }

  @override
  Widget build(BuildContext context) => Listener(
    onPointerDown: (_) => _set(true),
    onPointerUp: (_) => _set(false),
    onPointerCancel: (_) => _set(false),
    child: AnimatedScale(
      scale: _down ? 0.98 : 1,
      duration: Motion.fast,
      curve: Motion.emphasized,
      child: widget.child,
    ),
  );
}

/// An icon on a soft rounded square.
class IconTile extends StatelessWidget {
  const IconTile(
    this.icon, {
    super.key,
    this.tone = Tone.neutral,
    this.size = 40,
    this.gradient,
  });

  final IconData icon;
  final Tone tone;
  final double size;

  /// Paints the tile with a gradient and the icon white.
  final Gradient? gradient;

  @override
  Widget build(BuildContext context) {
    final (fg, bg) = tone.colors(context.talvio);
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: gradient == null ? bg : null,
        gradient: gradient,
        borderRadius: BorderRadius.circular(size * 0.32),
      ),
      alignment: Alignment.center,
      child: Icon(
        icon,
        size: size * 0.52,
        color: gradient == null ? fg : Colors.white,
      ),
    );
  }
}

/// A small round dot: online, offline, connecting, live.
class StatusDot extends StatelessWidget {
  const StatusDot({
    super.key,
    required this.color,
    this.size = 8,
    this.pulse = false,
  });

  final Color color;
  final double size;

  /// A soft ring that breathes around the dot.
  final bool pulse;

  @override
  Widget build(BuildContext context) {
    final dot = Container(
      width: size,
      height: size,
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    );
    if (!pulse) return dot;
    return _Breathing(
      builder: (t) => SizedBox.square(
        dimension: size * 2.2,
        child: Stack(
          alignment: Alignment.center,
          children: [
            Container(
              width: size * (1 + 1.2 * t),
              height: size * (1 + 1.2 * t),
              decoration: BoxDecoration(
                color: color.withValues(alpha: 0.35 * (1 - t)),
                shape: BoxShape.circle,
              ),
            ),
            dot,
          ],
        ),
      ),
    );
  }
}

/// A short label on a rounded pill.
class Pill extends StatelessWidget {
  const Pill(
    this.label, {
    super.key,
    this.tone = Tone.neutral,
    this.icon,
    this.dot,
  });

  final String label;
  final Tone tone;
  final IconData? icon;

  /// A coloured dot in front of the label.
  final Color? dot;

  @override
  Widget build(BuildContext context) {
    final (fg, bg) = tone.colors(context.talvio);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(Radii.pill),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (dot case final dot?) ...[
              StatusDot(color: dot, size: 6),
              const SizedBox(width: 6),
            ],
            if (icon case final icon?) ...[
              Icon(icon, size: 14, color: fg),
              const SizedBox(width: 4),
            ],
            Flexible(
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelMedium?.copyWith(color: fg),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// A section's heading: a quiet label, and an action at its end.
class SectionHeader extends StatelessWidget {
  const SectionHeader(this.title, {super.key, this.trailing, this.padding});

  final String title;
  final Widget? trailing;
  final EdgeInsetsGeometry? padding;

  @override
  Widget build(BuildContext context) => Padding(
    padding: padding ?? const EdgeInsets.fromLTRB(4, Space.xxl, 4, Space.sm),
    child: Row(
      children: [
        Expanded(
          child: Semantics(
            header: true,
            child: Text(
              title,
              style: context.text.titleSmall?.copyWith(
                color: context.talvio.muted,
              ),
            ),
          ),
        ),
        ?trailing,
      ],
    ),
  );
}

/// A message in the flow of a page: what happened, or what to do, with its
/// actions. Errors, offline computers, approval and tips all use it.
class Notice extends StatelessWidget {
  const Notice({
    super.key,
    required this.message,
    this.title,
    this.tone = Tone.info,
    this.icon,
    this.actions = const [],
    this.busy = false,
    this.live = true,
  });

  final String message;
  final String? title;
  final Tone tone;
  final IconData? icon;
  final List<Widget> actions;

  /// A spinner in place of the icon.
  final bool busy;

  /// Read out by screen readers when it appears.
  final bool live;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final (fg, bg) = tone.colors(t);
    final quiet = tone == Tone.info || tone == Tone.neutral;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: quiet ? t.card : bg,
        borderRadius: BorderRadius.circular(Radii.lg),
        border: Border.all(color: quiet ? t.hairline : bg),
      ),
      child: Padding(
        padding: EdgeInsets.fromLTRB(
          Space.lg,
          Space.lg,
          Space.lg,
          actions.isEmpty ? Space.lg : Space.sm,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox.square(
                  dimension: 22,
                  child: busy
                      ? Padding(
                          padding: const EdgeInsets.all(2),
                          child: CircularProgressIndicator(
                            strokeWidth: 2.2,
                            color: fg,
                          ),
                        )
                      : Icon(icon ?? _icon, size: 22, color: fg),
                ),
                const SizedBox(width: Space.md),
                Expanded(
                  child: Semantics(
                    liveRegion: live,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (title case final title?) ...[
                          Text(title, style: context.text.titleSmall),
                          const SizedBox(height: 2),
                        ],
                        Text(
                          message,
                          style: context.text.bodyMedium?.copyWith(
                            color: title == null ? t.ink : t.muted,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
            if (actions.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: Space.xs),
                child: OverflowBar(
                  alignment: MainAxisAlignment.end,
                  spacing: Space.xs,
                  children: actions,
                ),
              ),
          ],
        ),
      ),
    );
  }

  IconData get _icon => switch (tone) {
    Tone.danger => Icons.error_outline_rounded,
    Tone.warning => Icons.warning_amber_rounded,
    Tone.success => Icons.check_circle_outline_rounded,
    Tone.accent => Icons.auto_awesome_outlined,
    _ => Icons.info_outline_rounded,
  };
}

/// A page with nothing to show yet: a tile, a headline, a line or two, and
/// what to do about it.
class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.icon,
    required this.title,
    this.body,
    this.action,
    this.tone = Tone.accent,
    this.busy = false,
  });

  final IconData icon;
  final String title;
  final String? body;
  final Widget? action;
  final Tone tone;

  /// Waiting rather than empty: the tile breathes.
  final bool busy;

  @override
  Widget build(BuildContext context) {
    final tile = IconTile(icon, tone: tone, size: 64);
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 420),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (busy)
                _Breathing(
                  builder: (t) =>
                      Transform.scale(scale: 1 + 0.05 * t, child: tile),
                )
              else
                tile,
              const SizedBox(height: Space.xl),
              Text(
                title,
                textAlign: TextAlign.center,
                style: context.text.headlineSmall,
              ),
              if (body case final body?) ...[
                const SizedBox(height: Space.sm),
                Text(
                  body,
                  textAlign: TextAlign.center,
                  style: context.text.bodyMedium?.copyWith(
                    color: context.talvio.muted,
                  ),
                ),
              ],
              if (action case final action?) ...[
                const SizedBox(height: Space.xxl),
                action,
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// Three dots that rise in turn: the computer is at it.
class WorkingDots extends StatelessWidget {
  const WorkingDots({super.key, this.color, this.size = 6});

  final Color? color;
  final double size;

  @override
  Widget build(BuildContext context) {
    final c = color ?? context.talvio.accent;
    return ExcludeSemantics(
      child: _Looping(
        period: const Duration(milliseconds: 1100),
        builder: (t) => Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (var i = 0; i < 3; i++)
              Padding(
                padding: EdgeInsets.only(left: i == 0 ? 0 : size * 0.6),
                child: Transform.translate(
                  offset: Offset(0, -size * 0.6 * _bump(t - i * 0.15)),
                  child: Opacity(
                    opacity: 0.45 + 0.55 * _bump(t - i * 0.15),
                    child: Container(
                      width: size,
                      height: size,
                      decoration: BoxDecoration(
                        color: c,
                        shape: BoxShape.circle,
                      ),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  static double _bump(double t) {
    final x = (t % 1 + 1) % 1;
    return x < 0.4 ? math.sin(x / 0.4 * math.pi) : 0;
  }
}

/// A child rebuilt with a value going 0 → 1 → 0, forever.
class _Breathing extends StatefulWidget {
  const _Breathing({required this.builder});

  final Widget Function(double t) builder;

  @override
  State<_Breathing> createState() => _BreathingState();
}

class _BreathingState extends State<_Breathing>
    with SingleTickerProviderStateMixin {
  late final _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1400),
  );

  @override
  void initState() {
    super.initState();
    if (!_reduced) unawaited(_c.repeat(reverse: true));
  }

  bool get _reduced =>
      !ambientMotion ||
      WidgetsBinding
          .instance
          .platformDispatcher
          .accessibilityFeatures
          .disableAnimations;

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, _) =>
        widget.builder(Motion.standard.transform(_c.value)),
  );
}

/// A child rebuilt with a value going 0 → 1, forever.
class _Looping extends StatefulWidget {
  const _Looping({required this.builder, required this.period});

  final Widget Function(double t) builder;
  final Duration period;

  @override
  State<_Looping> createState() => _LoopingState();
}

class _LoopingState extends State<_Looping>
    with SingleTickerProviderStateMixin {
  late final _c = AnimationController(
    vsync: this,
    duration: widget.period,
    value: 0.3,
  );

  @override
  void initState() {
    super.initState();
    if (ambientMotion) unawaited(_c.repeat());
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, _) => widget.builder(_c.value),
  );
}

/// The wait while the computer asks its user to allow this phone: a laptop
/// with rings going out from it, what to do, and Cancel.
class ApprovalCard extends StatelessWidget {
  const ApprovalCard({
    super.key,
    required this.title,
    required this.body,
    this.onCancel,
    this.cancelLabel,
  });

  final String title;
  final String body;
  final VoidCallback? onCancel;
  final String? cancelLabel;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Surface(
      padding: const EdgeInsets.fromLTRB(
        Space.xl,
        Space.xl,
        Space.xl,
        Space.md,
      ),
      child: Column(
        children: [
          SizedBox.square(
            dimension: 88,
            child: _Looping(
              period: const Duration(milliseconds: 2200),
              builder: (v) => CustomPaint(
                painter: _RingsPainter(t.accent, v),
                child: Center(
                  child: IconTile(
                    Icons.laptop_chromebook_outlined,
                    size: 52,
                    gradient: TalvioColors.gradient,
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(height: Space.lg),
          Semantics(
            liveRegion: true,
            child: Text(
              title,
              textAlign: TextAlign.center,
              style: context.text.titleLarge,
            ),
          ),
          const SizedBox(height: Space.xs),
          Text(
            body,
            textAlign: TextAlign.center,
            style: context.text.bodyMedium?.copyWith(color: t.muted),
          ),
          if (onCancel != null || cancelLabel != null) ...[
            const SizedBox(height: Space.sm),
            TextButton(onPressed: onCancel, child: Text(cancelLabel ?? '')),
          ] else
            const SizedBox(height: Space.sm),
        ],
      ),
    );
  }
}

class _RingsPainter extends CustomPainter {
  const _RingsPainter(this.color, this.t);

  final Color color;
  final double t;

  @override
  void paint(Canvas canvas, Size size) {
    final center = size.center(Offset.zero);
    for (var i = 0; i < 2; i++) {
      final p = (t + i * 0.5) % 1;
      canvas.drawCircle(
        center,
        26 + p * (size.width / 2 - 26),
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 2
          ..color = color.withValues(alpha: 0.5 * (1 - p)),
      );
    }
  }

  @override
  bool shouldRepaint(_RingsPainter old) => old.t != t || old.color != color;
}

/// The progress indicator inside a button whose action is running.
class ButtonSpinner extends StatelessWidget {
  const ButtonSpinner({super.key, this.color});

  final Color? color;

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: 20,
    child: CircularProgressIndicator(strokeWidth: 2, color: color),
  );
}

/// Two or three choices on a track, the chosen one raised: a quieter
/// segmented control than Material's, for settings that sit in a card.
class SegmentedPill<T> extends StatelessWidget {
  const SegmentedPill({
    super.key,
    required this.options,
    required this.selected,
    required this.onChanged,
  });

  /// Each choice with its label (and an optional icon).
  final List<(T value, String label, IconData? icon)> options;
  final T selected;

  /// Null: the control shows the choice but cannot change it now.
  final ValueChanged<T>? onChanged;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = onChanged != null;
    final index = options.indexWhere((o) => o.$1 == selected);
    return Container(
      height: 44,
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: t.sunken,
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final width = constraints.maxWidth / options.length;
          return Stack(
            children: [
              if (index >= 0)
                AnimatedPositioned(
                  duration: Motion.medium,
                  curve: Motion.emphasized,
                  left: width * index,
                  top: 0,
                  bottom: 0,
                  width: width,
                  child: Container(
                    decoration: BoxDecoration(
                      color: t.card,
                      borderRadius: BorderRadius.circular(Radii.md - 3),
                      boxShadow: [
                        BoxShadow(
                          color: Colors.black.withValues(alpha: 0.08),
                          blurRadius: 6,
                          offset: const Offset(0, 1),
                        ),
                      ],
                    ),
                  ),
                ),
              Row(
                children: [
                  for (final (value, label, icon) in options)
                    Expanded(
                      child: Semantics(
                        button: true,
                        selected: value == selected,
                        enabled: enabled,
                        child: InkWell(
                          borderRadius: BorderRadius.circular(Radii.md - 3),
                          onTap: !enabled || value == selected
                              ? null
                              : () {
                                  Haptic.tick();
                                  onChanged!(value);
                                },
                          child: Center(
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                if (icon != null) ...[
                                  Icon(
                                    icon,
                                    size: 17,
                                    color: value == selected
                                        ? t.accent
                                        : t.muted,
                                  ),
                                  const SizedBox(width: 6),
                                ],
                                Flexible(
                                  child: Text(
                                    label,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: context.text.labelLarge?.copyWith(
                                      color: !enabled
                                          ? t.faint
                                          : value == selected
                                          ? t.ink
                                          : t.muted,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ],
          );
        },
      ),
    );
  }
}

/// A scrolling list whose bottom edge fades out into the controls under it,
/// so content scrolls away rather than being cut.
class FadeBottom extends StatelessWidget {
  const FadeBottom({super.key, required this.child, this.size = 24});

  final Widget child;
  final double size;

  @override
  Widget build(BuildContext context) => ShaderMask(
    blendMode: BlendMode.dstIn,
    shaderCallback: (rect) => LinearGradient(
      begin: Alignment.topCenter,
      end: Alignment.bottomCenter,
      colors: const [Colors.black, Colors.black, Colors.transparent],
      stops: [0, rect.height <= size ? 0 : 1 - size / rect.height, 1],
    ).createShader(rect),
    child: child,
  );
}

/// Fades and lifts its child in once, when it first appears: a new answer,
/// a result, a page.
class Appear extends StatelessWidget {
  const Appear({super.key, required this.child, this.offset = 10});

  final Widget child;

  /// How far below its place it starts.
  final double offset;

  @override
  Widget build(BuildContext context) => TweenAnimationBuilder<double>(
    tween: Tween(begin: 0, end: 1),
    duration: Motion.medium,
    curve: Motion.emphasized,
    builder: (context, t, child) => Opacity(
      opacity: t,
      child: Transform.translate(
        offset: Offset(0, offset * (1 - t)),
        child: child,
      ),
    ),
    child: child,
  );
}
