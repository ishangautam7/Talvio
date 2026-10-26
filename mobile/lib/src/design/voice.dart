// What every page that listens draws: the live waveform, the recording's
// clock, the Dictate mic and the composer (a text field with the mic in
// it), design spec §3.3 and §4.5.

import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'components.dart';
import 'tokens.dart';

/// A recording as a page shows it.
class VoiceState {
  const VoiceState({
    required this.recording,
    required this.paused,
    required this.level,
    required this.elapsed,
    required this.onStart,
    required this.onStop,
    required this.onDiscard,
    required this.startLabel,
    required this.stopLabel,
    this.discardLabel = 'Discard',
    this.pausedLabel = 'Paused',
  });

  final bool recording;

  /// Paused by a call or another app's audio.
  final bool paused;
  final ValueListenable<double> level;
  final ValueListenable<Duration> elapsed;

  /// Null while recording cannot start (offline, busy).
  final VoidCallback? onStart;
  final VoidCallback onStop;
  final VoidCallback onDiscard;

  /// What a screen reader says the mic does, idle and recording.
  final String startLabel;
  final String stopLabel;
  final String discardLabel;
  final String pausedLabel;
}

/// `m:ss`.
String clock(Duration d) =>
    '${d.inMinutes}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';

/// The last few seconds of input level as bars, newest at the end: 3 wide,
/// 2 apart, 4 to 24 tall.
class LiveWaveform extends StatefulWidget {
  const LiveWaveform({
    super.key,
    required this.level,
    this.height = 24,
    this.color,
    this.active = true,
  });

  final ValueListenable<double> level;
  final double height;
  final Color? color;

  /// False while paused: the bars sit flat and faint.
  final bool active;

  @override
  State<LiveWaveform> createState() => _LiveWaveformState();
}

class _LiveWaveformState extends State<LiveWaveform> {
  static const _keep = 96;
  var _history = List<double>.filled(_keep, 0);

  @override
  void initState() {
    super.initState();
    widget.level.addListener(_onLevel);
  }

  @override
  void didUpdateWidget(LiveWaveform old) {
    super.didUpdateWidget(old);
    if (old.level != widget.level) {
      old.level.removeListener(_onLevel);
      widget.level.addListener(_onLevel);
    }
  }

  @override
  void dispose() {
    widget.level.removeListener(_onLevel);
    super.dispose();
  }

  void _onLevel() => setState(() {
    _history = [..._history.skip(1), widget.active ? widget.level.value : 0];
  });

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return ExcludeSemantics(
      child: SizedBox(
        height: widget.height,
        child: CustomPaint(
          painter: _BarsPainter(
            _history,
            widget.active ? (widget.color ?? t.coral) : t.faint,
            Directionality.of(context) == TextDirection.rtl,
          ),
          size: Size.infinite,
        ),
      ),
    );
  }
}

class _BarsPainter extends CustomPainter {
  const _BarsPainter(this.values, this.color, this.rtl);

  final List<double> values;
  final Color color;
  final bool rtl;

  static const _bar = 3.0;
  static const _gap = 2.0;

  @override
  void paint(Canvas canvas, Size size) {
    final n = math.min(values.length, ((size.width + _gap) / 5).floor());
    if (n <= 0) return;
    final shown = values.sublist(values.length - n);
    final paint = Paint()..color = color;
    final maxH = math.min(24.0, size.height);
    for (var i = 0; i < n; i++) {
      final h = 4 + (maxH - 4) * shown[i].clamp(0.0, 1.0);
      final slot = rtl ? n - 1 - i : i;
      final x = size.width - (n - slot) * (_bar + _gap) + _gap;
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromLTWH(x, (size.height - h) / 2, _bar, h),
          const Radius.circular(1.5),
        ),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(_BarsPainter old) => true;
}

/// The Dictate mic: 80 across, the brand gradient. While recording a stop
/// square, and a coral halo that follows the voice.
class MicOrb extends StatefulWidget {
  const MicOrb({super.key, required this.voice, this.size = 80});

  final VoiceState voice;
  final double size;

  @override
  State<MicOrb> createState() => _MicOrbState();
}

class _MicOrbState extends State<MicOrb> {
  var _down = false;

  @override
  Widget build(BuildContext context) {
    final voice = widget.voice;
    final size = widget.size;
    final recording = voice.recording;
    final onPressed = recording
        ? () {
            Haptic.firm();
            voice.onStop();
          }
        : voice.onStart == null
        ? null
        : () {
            Haptic.tap();
            voice.onStart!();
          };
    final enabled = onPressed != null;
    final t = context.talvio;
    final moving = motionAllowed(context);
    // The halo and ripples spill past the orb's own box; the layout is the
    // orb.
    return SizedBox.square(
      dimension: size,
      child: Stack(
        alignment: Alignment.center,
        clipBehavior: Clip.none,
        children: [
          if (recording && !voice.paused)
            Positioned(
              left: -size * 0.6,
              right: -size * 0.6,
              top: -size * 0.6,
              bottom: -size * 0.6,
              child: _Ripples(color: t.coral, size: size),
            ),
          Semantics(
            label: recording ? voice.stopLabel : voice.startLabel,
            button: true,
            enabled: enabled,
            onTap: onPressed,
            excludeSemantics: true,
            child: GestureDetector(
              onTapDown: enabled ? (_) => setState(() => _down = true) : null,
              onTapUp: enabled ? (_) => setState(() => _down = false) : null,
              onTapCancel: enabled ? () => setState(() => _down = false) : null,
              onTap: onPressed,
              child: AnimatedScale(
                scale: _down && moving ? 0.95 : 1,
                duration: Motion.press,
                curve: Motion.pressCurve,
                child: AnimatedOpacity(
                  duration: Motion.quick,
                  opacity: enabled ? 1 : 0.38,
                  child: Stack(
                    alignment: Alignment.center,
                    clipBehavior: Clip.none,
                    children: [
                      TalvioOrb(
                        size: size,
                        mood: recording && !voice.paused
                            ? OrbMood.listening
                            : OrbMood.idle,
                        level: voice.level,
                        halo: enabled,
                      ),
                      AnimatedSwitcher(
                        duration: Motion.quick,
                        transitionBuilder: _swap,
                        child: Icon(
                          recording ? AppIcons.stop : AppIcons.micFill,
                          key: ValueKey(recording),
                          size: recording ? 26 : 30,
                          color: Colors.white,
                          shadows: const [
                            Shadow(color: Color(0x40000000), blurRadius: 8),
                          ],
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
    );
  }
}

/// Rings that leave the recording orb and fade, like sound going out.
/// Still under reduced motion: one quiet ring.
class _Ripples extends StatelessWidget {
  const _Ripples({required this.color, required this.size});

  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (!loopsAllowed(context)) {
      return CustomPaint(painter: _RipplePainter(color, size, const [0.3]));
    }
    return Looping(
      period: const Duration(milliseconds: 2400),
      builder: (t) =>
          CustomPaint(painter: _RipplePainter(color, size, [t, (t + 0.5) % 1])),
    );
  }
}

class _RipplePainter extends CustomPainter {
  const _RipplePainter(this.color, this.size, this.phases);

  final Color color;
  final double size;
  final List<double> phases;

  @override
  void paint(Canvas canvas, Size box) {
    final c = box.center(Offset.zero);
    for (final p in phases) {
      final r = size / 2 * (1.08 + 0.55 * p);
      canvas.drawCircle(
        c,
        r,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.5
          ..color = color.withValues(alpha: 0.45 * (1 - p)),
      );
    }
  }

  @override
  bool shouldRepaint(_RipplePainter old) => true;
}

/// The composer's content swap: a fade and a slight grow.
Widget _swap(Widget child, Animation<double> animation) => FadeTransition(
  opacity: animation,
  child: ScaleTransition(
    scale: Tween(begin: 0.9, end: 1.0).animate(animation),
    child: child,
  ),
);

/// Under the Dictate mic while recording: Discard, the waveform and the
/// clock (or Paused).
class RecordingStrip extends StatelessWidget {
  const RecordingStrip({super.key, required this.voice});

  final VoiceState voice;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        IconBtn(
          icon: AppIcons.discard,
          tooltip: voice.discardLabel,
          color: t.muted,
          onPressed: () {
            Haptic.tick();
            voice.onDiscard();
          },
        ),
        const SizedBox(width: Space.s8),
        Flexible(
          child: SizedBox(
            width: 180,
            child: LiveWaveform(level: voice.level, active: !voice.paused),
          ),
        ),
        const SizedBox(width: Space.s12),
        _Clock(voice: voice),
      ],
    );
  }
}

class _Clock extends StatelessWidget {
  const _Clock({required this.voice});

  final VoiceState voice;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return ValueListenableBuilder(
      valueListenable: voice.elapsed,
      builder: (context, value, _) => Text(
        voice.paused ? voice.pausedLabel : clock(value),
        textScaler: MediaQuery.textScalerOf(context).clamp(maxScaleFactor: 1.2),
        style: voice.paused
            ? TalvioType.footnote.copyWith(color: t.warning)
            : TalvioType.mono.copyWith(color: t.muted),
      ),
    );
  }
}

/// A round 40 button inside the composer.
class ComposerButton extends StatelessWidget {
  const ComposerButton({
    super.key,
    required this.label,
    required this.icon,
    required this.onPressed,
    this.background,
    this.foreground,
    this.iconSize = 20,
    this.active = false,
    this.brand = false,
  });

  final String label;
  final IconData icon;
  final VoidCallback? onPressed;

  /// Null: transparent (the Keys toggle).
  final Color? background;
  final Color? foreground;
  final double iconSize;

  /// The Keys toggle is on: an ink glyph on a fill circle.
  final bool active;

  /// The mic: the brand gradient and a soft coral glow instead of the flat
  /// accent (the screen's one primary action).
  final bool brand;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = onPressed != null;
    final bg = !enabled && background != null
        ? t.fill
        : active
        ? t.fill
        : background;
    final fg = !enabled && background != null
        ? t.muted.withValues(alpha: 0.6)
        : active
        ? t.ink
        : foreground ?? t.muted;
    return Semantics(
      label: label,
      button: true,
      enabled: enabled,
      selected: background == null ? active : null,
      onTap: onPressed,
      excludeSemantics: true,
      child: Tooltip(
        message: label,
        excludeFromSemantics: true,
        child: Pressable(
          enabled: enabled,
          child: DecoratedBox(
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              boxShadow: brand && enabled
                  ? [
                      BoxShadow(
                        color: t.coral.withValues(
                          alpha: t.isDark ? 0.45 : 0.35,
                        ),
                        blurRadius: 14,
                        offset: const Offset(0, 4),
                      ),
                    ]
                  : null,
            ),
            child: Material(
              color: brand && enabled ? null : bg ?? Colors.transparent,
              type: brand && enabled
                  ? MaterialType.transparency
                  : MaterialType.canvas,
              shape: const CircleBorder(),
              clipBehavior: Clip.antiAlias,
              child: InkWell(
                onTap: onPressed,
                highlightColor: background == t.accent
                    ? t.pressedOnAccent
                    : t.pressed,
                child: Ink(
                  width: 40,
                  height: 40,
                  decoration: brand && enabled
                      ? BoxDecoration(
                          shape: BoxShape.circle,
                          gradient: TalvioColors.gradient,
                        )
                      : null,
                  child: Center(
                    child: Opacity(
                      opacity: !enabled && background == null ? 0.38 : 1,
                      child: Icon(icon, size: iconSize, color: fg),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// The bar at the bottom of a page: type, or tap the mic and speak. While
/// recording it turns into the waveform with Discard and Send; while a
/// request is out its button stops it.
class Composer extends StatefulWidget {
  const Composer({
    super.key,
    required this.hint,
    required this.sendLabel,
    required this.onSubmit,
    this.voice,
    this.enabled = true,
    this.busy = false,
    this.controller,
    this.focusNode,
    this.inputFormatters,
    this.maxLines = 6,
    this.leading,
    this.clearOnSubmit = true,
    this.onCancel,
    this.cancelLabel = 'Cancel',
    this.onEnter,
    this.enterLabel = 'Press Enter',
    this.margin = const EdgeInsets.fromLTRB(16, 8, 16, 8),
  });

  final String hint;

  /// What a screen reader says the send button does.
  final String sendLabel;

  /// Sends the typed text; true when it was taken (the field then clears).
  final Future<bool> Function(String text) onSubmit;

  /// Null: a text-only composer.
  final VoiceState? voice;

  /// Typing and sending are possible.
  final bool enabled;

  /// A request is on its way.
  final bool busy;
  final TextEditingController? controller;
  final FocusNode? focusNode;
  final List<TextInputFormatter>? inputFormatters;
  final int maxLines;

  /// Before the field (the Keys toggle).
  final Widget? leading;
  final bool clearOnSubmit;

  /// While [busy]: the stop button, which asks the computer to stop.
  final VoidCallback? onCancel;
  final String cancelLabel;

  /// With an empty field and no voice: Enter, pressed on the computer.
  final VoidCallback? onEnter;
  final String enterLabel;
  final EdgeInsetsGeometry margin;

  @override
  State<Composer> createState() => _ComposerState();
}

class _ComposerState extends State<Composer> {
  TextEditingController? _ownText;
  FocusNode? _ownFocus;

  TextEditingController get _text =>
      widget.controller ?? (_ownText ??= TextEditingController());
  FocusNode get _focus => widget.focusNode ?? (_ownFocus ??= FocusNode());

  @override
  void dispose() {
    _ownText?.dispose();
    _ownFocus?.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final text = _text.text.trim();
    if (text.isEmpty || !widget.enabled || widget.busy) return;
    Haptic.tap();
    if (await widget.onSubmit(text) && mounted && widget.clearOnSubmit) {
      _text.clear();
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final voice = widget.voice;
    final recording = voice?.recording ?? false;
    return Padding(
      padding: widget.margin,
      child: ValueListenableBuilder(
        valueListenable: _text,
        builder: (context, value, _) => LayoutBuilder(
          builder: (context, constraints) {
            final multiline =
                !recording && _overflows(context, value.text, constraints);
            return AnimatedContainer(
              duration: Motion.quick,
              constraints: const BoxConstraints(minHeight: 48),
              decoration: BoxDecoration(
                color: t.card,
                borderRadius: BorderRadius.circular(multiline ? 20 : 24),
                border: Border.all(color: t.isDark ? t.border : t.hairline),
                boxShadow: Depth.card(t),
              ),
              child: AnimatedSwitcher(
                duration: Motion.quick,
                transitionBuilder: _swap,
                child: recording
                    ? _recording(voice!)
                    : _typing(context, value.text),
              ),
            );
          },
        ),
      ),
    );
  }

  /// Whether [text] runs past one line in the field.
  bool _overflows(BuildContext context, String text, BoxConstraints c) {
    if (text.isEmpty) return false;
    if (text.contains('\n')) return true;
    final room = c.maxWidth - (widget.leading == null ? 16 : 48) - 4 - 48 - 2;
    final painter = TextPainter(
      text: TextSpan(text: text, style: TalvioType.body),
      maxLines: 1,
      textDirection: Directionality.of(context),
      textScaler: MediaQuery.textScalerOf(context),
    )..layout(maxWidth: math.max(1, room));
    final over = painter.didExceedMaxLines;
    painter.dispose();
    return over;
  }

  Widget _recording(VoiceState voice) {
    final t = context.talvio;
    return Padding(
      key: const ValueKey('recording'),
      padding: const EdgeInsets.all(4),
      child: Row(
        children: [
          ComposerButton(
            label: voice.discardLabel,
            icon: AppIcons.discard,
            onPressed: () {
              Haptic.tick();
              voice.onDiscard();
            },
          ),
          const SizedBox(width: Space.s8),
          Expanded(
            child: LiveWaveform(level: voice.level, active: !voice.paused),
          ),
          const SizedBox(width: Space.s12),
          _Clock(voice: voice),
          const SizedBox(width: Space.s8),
          ComposerButton(
            label: voice.stopLabel,
            icon: AppIcons.send,
            background: t.accent,
            foreground: t.onAccent,
            onPressed: () {
              Haptic.firm();
              voice.onStop();
            },
          ),
        ],
      ),
    );
  }

  Widget _typing(BuildContext context, String text) {
    final t = context.talvio;
    final voice = widget.voice;
    final hasText = text.trim().isNotEmpty;
    final Widget trailing;
    if (widget.busy) {
      final cancel = widget.onCancel;
      trailing = ComposerButton(
        key: const ValueKey('stop'),
        label: widget.cancelLabel,
        icon: AppIcons.stop,
        iconSize: 16,
        background: t.ink,
        foreground: t.canvas,
        onPressed: cancel == null
            ? null
            : () {
                Haptic.firm();
                cancel();
              },
      );
    } else if (hasText || (voice == null && widget.onEnter == null)) {
      trailing = ComposerButton(
        key: const ValueKey('send'),
        label: widget.sendLabel,
        icon: AppIcons.send,
        background: t.ink,
        foreground: t.canvas,
        onPressed: widget.enabled && hasText ? _submit : null,
      );
    } else if (voice == null) {
      final enter = widget.onEnter!;
      trailing = ComposerButton(
        key: const ValueKey('enter'),
        label: widget.enterLabel,
        icon: AppIcons.enter,
        background: t.fill,
        foreground: t.ink,
        onPressed: widget.enabled ? enter : null,
      );
    } else {
      trailing = ComposerButton(
        key: const ValueKey('mic'),
        label: voice.startLabel,
        icon: AppIcons.micFill,
        background: t.accent,
        foreground: Colors.white,
        brand: true,
        onPressed: voice.onStart == null
            ? null
            : () {
                Haptic.tap();
                _focus.unfocus();
                voice.onStart!();
              },
      );
    }
    return Row(
      key: const ValueKey('typing'),
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        if (widget.leading case final leading?)
          Padding(padding: const EdgeInsets.all(4), child: leading),
        Expanded(
          child: Padding(
            padding: EdgeInsetsDirectional.only(
              start: widget.leading == null ? 16 : 4,
              end: 4,
            ),
            child: TextField(
              controller: _text,
              focusNode: _focus,
              enabled: widget.enabled && !widget.busy,
              minLines: 1,
              maxLines: widget.maxLines,
              inputFormatters: widget.inputFormatters,
              textCapitalization: TextCapitalization.sentences,
              onTapOutside: (_) => _focus.unfocus(),
              style: TalvioType.body.copyWith(
                color: widget.enabled ? t.ink : t.faint,
              ),
              cursorColor: t.accent,
              decoration: InputDecoration(
                hintText: widget.hint,
                hintMaxLines: 1,
                filled: false,
                isDense: true,
                border: InputBorder.none,
                enabledBorder: InputBorder.none,
                focusedBorder: InputBorder.none,
                disabledBorder: InputBorder.none,
                contentPadding: const EdgeInsets.symmetric(vertical: 12),
                hintStyle: TalvioType.body.copyWith(color: t.faint),
              ),
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.all(4),
          child: AnimatedSwitcher(
            duration: Motion.quick,
            transitionBuilder: _swap,
            child: trailing,
          ),
        ),
      ],
    );
  }
}
