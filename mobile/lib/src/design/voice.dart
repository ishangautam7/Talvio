// What every page that listens draws: the live waveform, the recording's
// clock, the big mic and the composer (a text field with the mic in it).

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

/// The last few seconds of input level as bars, newest on the right.
class LiveWaveform extends StatefulWidget {
  const LiveWaveform({
    super.key,
    required this.level,
    this.bars = 28,
    this.height = 28,
    this.color,
    this.active = true,
  });

  final ValueListenable<double> level;
  final int bars;
  final double height;
  final Color? color;

  /// False while paused: the bars sit flat.
  final bool active;

  @override
  State<LiveWaveform> createState() => _LiveWaveformState();
}

class _LiveWaveformState extends State<LiveWaveform> {
  late List<double> _history = List.filled(widget.bars, 0);

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
    if (old.bars != widget.bars) _history = List.filled(widget.bars, 0);
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
    final color = widget.color ?? context.talvio.accent;
    return ExcludeSemantics(
      child: SizedBox(
        height: widget.height,
        child: CustomPaint(
          painter: _BarsPainter(_history, color),
          size: Size.infinite,
        ),
      ),
    );
  }
}

class _BarsPainter extends CustomPainter {
  const _BarsPainter(this.values, this.color);

  final List<double> values;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final n = values.length;
    final slot = size.width / n;
    final w = math.min(4.0, slot * 0.55);
    final paint = Paint()..strokeCap = StrokeCap.round;
    for (var i = 0; i < n; i++) {
      final v = values[i];
      final h = math.max(w, size.height * (0.12 + 0.88 * v));
      final x = slot * i + slot / 2;
      paint
        ..color = color.withValues(alpha: 0.35 + 0.65 * (i / n))
        ..strokeWidth = w;
      canvas.drawLine(
        Offset(x, (size.height - h) / 2 + w / 2),
        Offset(x, (size.height + h) / 2 - w / 2),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(_BarsPainter old) => true;
}

/// The round mic: the brand gradient, a halo that follows the voice while
/// recording, and a stop square then.
class MicOrb extends StatelessWidget {
  const MicOrb({
    super.key,
    required this.voice,
    this.size = 84,
    this.busy = false,
  });

  final VoiceState voice;
  final double size;

  /// A request is on its way: a spinner, and no taps.
  final bool busy;

  @override
  Widget build(BuildContext context) {
    final recording = voice.recording;
    final onPressed = busy
        ? null
        : recording
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
    final halo = size * 1.5;
    return SizedBox.square(
      dimension: halo,
      child: Stack(
        alignment: Alignment.center,
        children: [
          if (recording)
            ValueListenableBuilder(
              valueListenable: voice.level,
              builder: (context, value, _) => AnimatedContainer(
                duration: const Duration(milliseconds: 120),
                width: size + (halo - size) * (voice.paused ? 0 : value),
                height: size + (halo - size) * (voice.paused ? 0 : value),
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: t.coral.withValues(alpha: 0.18),
                ),
              ),
            ),
          Semantics(
            label: recording ? voice.stopLabel : voice.startLabel,
            button: true,
            enabled: enabled,
            onTap: onPressed,
            excludeSemantics: true,
            child: AnimatedContainer(
              duration: Motion.medium,
              curve: Motion.emphasized,
              width: size,
              height: size,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                gradient: enabled || busy ? TalvioColors.gradient : null,
                color: enabled || busy ? null : t.sunken,
                boxShadow: enabled
                    ? [
                        BoxShadow(
                          color: t.coral.withValues(alpha: 0.32),
                          blurRadius: 24,
                          offset: const Offset(0, 8),
                        ),
                      ]
                    : null,
              ),
              child: Material(
                type: MaterialType.transparency,
                shape: const CircleBorder(),
                clipBehavior: Clip.antiAlias,
                child: InkWell(
                  onTap: onPressed,
                  child: Center(
                    child: busy
                        ? SizedBox.square(
                            dimension: size * 0.34,
                            child: const CircularProgressIndicator(
                              strokeWidth: 3,
                              color: Colors.white,
                            ),
                          )
                        : AnimatedSwitcher(
                            duration: Motion.fast,
                            child: Icon(
                              recording
                                  ? Icons.stop_rounded
                                  : Icons.mic_rounded,
                              key: ValueKey(recording),
                              size: size * 0.42,
                              color: enabled ? Colors.white : t.faint,
                            ),
                          ),
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

/// While recording: Discard, the waveform and the clock (or Paused).
class RecordingStrip extends StatelessWidget {
  const RecordingStrip({super.key, required this.voice, this.trailingHint});

  final VoiceState voice;

  /// After the clock: what tapping the mic again does.
  final String? trailingHint;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    // Discard is an icon and the clock stops growing at 140%, so large text
    // leaves the waveform room rather than overflowing the row.
    return Row(
      children: [
        IconButton(
          tooltip: voice.discardLabel,
          onPressed: () {
            Haptic.tick();
            voice.onDiscard();
          },
          style: IconButton.styleFrom(
            backgroundColor: t.sunken,
            foregroundColor: t.muted,
          ),
          icon: const Icon(Icons.delete_outline_rounded),
        ),
        const SizedBox(width: Space.sm),
        Expanded(
          child: LiveWaveform(level: voice.level, active: !voice.paused),
        ),
        const SizedBox(width: Space.md),
        ValueListenableBuilder(
          valueListenable: voice.elapsed,
          builder: (context, value, _) => Text(
            voice.paused
                ? voice.pausedLabel
                : trailingHint == null
                ? clock(value)
                : '${clock(value)} · $trailingHint',
            textScaler: MediaQuery.textScalerOf(
              context,
            ).clamp(maxScaleFactor: 1.4),
            style: context.text.labelLarge?.copyWith(
              color: voice.paused ? t.warning : t.ink,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ),
        const SizedBox(width: Space.sm),
      ],
    );
  }
}

/// The bar at the bottom of a page: type, or tap the mic and speak. While
/// recording it turns into the waveform with Discard and Send; while a
/// request is out it waits.
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
    this.maxLines = 5,
    this.leading,
    this.clearOnSubmit = true,
    this.onCancel,
    this.cancelLabel = 'Cancel',
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

  /// Before the field (a Keys button).
  final Widget? leading;
  final bool clearOnSubmit;

  /// While [busy]: the stop button, which asks the computer to stop.
  final VoidCallback? onCancel;
  final String cancelLabel;

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
    return AnimatedContainer(
      duration: Motion.medium,
      curve: Motion.emphasized,
      constraints: const BoxConstraints(minHeight: 60),
      decoration: BoxDecoration(
        color: t.card,
        borderRadius: BorderRadius.circular(Radii.xl),
        border: Border.all(
          color: recording ? t.coral.withValues(alpha: 0.6) : t.hairline,
        ),
        boxShadow: [
          BoxShadow(
            color: const Color(0xFF1B1D33).withValues(alpha: 0.05),
            blurRadius: 12,
            offset: const Offset(0, 2),
          ),
        ],
      ),
      padding: const EdgeInsets.fromLTRB(6, 6, 6, 6),
      child: AnimatedSwitcher(
        duration: Motion.medium,
        switchInCurve: Motion.emphasized,
        child: recording ? _recording(voice!) : _typing(context),
      ),
    );
  }

  Widget _recording(VoiceState voice) => Row(
    key: const ValueKey('recording'),
    children: [
      Expanded(child: RecordingStrip(voice: voice)),
      _RoundButton(
        label: voice.stopLabel,
        icon: Icons.arrow_upward_rounded,
        gradient: true,
        onPressed: () {
          Haptic.firm();
          voice.onStop();
        },
      ),
    ],
  );

  Widget _typing(BuildContext context) {
    final t = context.talvio;
    final voice = widget.voice;
    return Row(
      key: const ValueKey('typing'),
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        if (widget.leading case final leading?) ...[
          leading,
          const SizedBox(width: 2),
        ],
        Expanded(
          child: Padding(
            padding: EdgeInsets.only(left: widget.leading == null ? 12 : 4),
            child: TextField(
              controller: _text,
              focusNode: _focus,
              enabled: widget.enabled && !widget.busy,
              minLines: 1,
              maxLines: widget.maxLines,
              inputFormatters: widget.inputFormatters,
              textCapitalization: TextCapitalization.sentences,
              onTapOutside: (_) => _focus.unfocus(),
              style: context.text.bodyLarge,
              decoration: InputDecoration(
                hintText: widget.hint,
                filled: false,
                isDense: true,
                border: InputBorder.none,
                enabledBorder: InputBorder.none,
                focusedBorder: InputBorder.none,
                disabledBorder: InputBorder.none,
                contentPadding: const EdgeInsets.symmetric(vertical: 13),
                hintStyle: context.text.bodyLarge?.copyWith(color: t.faint),
              ),
            ),
          ),
        ),
        const SizedBox(width: 6),
        ValueListenableBuilder(
          valueListenable: _text,
          builder: (context, value, _) {
            final hasText = value.text.trim().isNotEmpty;
            if (widget.busy) {
              final cancel = widget.onCancel;
              if (cancel == null) {
                return const _RoundButton(label: null, busy: true);
              }
              return _RoundButton(
                label: widget.cancelLabel,
                icon: Icons.stop_rounded,
                onPressed: () {
                  Haptic.firm();
                  cancel();
                },
              );
            }
            if (hasText || voice == null) {
              return _RoundButton(
                label: widget.sendLabel,
                icon: Icons.arrow_upward_rounded,
                onPressed: widget.enabled && hasText ? _submit : null,
              );
            }
            return _RoundButton(
              label: voice.startLabel,
              icon: Icons.mic_rounded,
              gradient: true,
              onPressed: voice.onStart == null
                  ? null
                  : () {
                      Haptic.tap();
                      _focus.unfocus();
                      voice.onStart!();
                    },
            );
          },
        ),
      ],
    );
  }
}

class _RoundButton extends StatelessWidget {
  const _RoundButton({
    required this.label,
    this.icon,
    this.onPressed,
    this.gradient = false,
    this.busy = false,
  });

  final String? label;
  final IconData? icon;
  final VoidCallback? onPressed;
  final bool gradient;
  final bool busy;

  static const size = 48.0;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = onPressed != null;
    final Widget button = AnimatedContainer(
      duration: Motion.fast,
      width: size,
      height: size,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        gradient: enabled && gradient ? TalvioColors.gradient : null,
        color: enabled && !gradient
            ? t.ink
            : enabled
            ? null
            : t.sunken,
      ),
      child: Material(
        type: MaterialType.transparency,
        shape: const CircleBorder(),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onPressed,
          child: Center(
            child: busy
                ? SizedBox.square(
                    dimension: 20,
                    child: CircularProgressIndicator(
                      strokeWidth: 2.2,
                      color: t.muted,
                    ),
                  )
                : Icon(
                    icon,
                    size: 24,
                    color: !enabled
                        ? t.faint
                        : gradient
                        ? Colors.white
                        : t.canvas,
                  ),
          ),
        ),
      ),
    );
    if (label == null) return button;
    return Semantics(
      label: label,
      button: true,
      enabled: enabled,
      onTap: onPressed,
      excludeSemantics: true,
      child: Tooltip(message: label!, child: button),
    );
  }
}
