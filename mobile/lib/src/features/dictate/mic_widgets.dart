// The big mic and the line under it while recording, as every page that
// records shows them (Send, Do).

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'computer_widgets.dart' show mutedText;
import 'strings.dart';

/// A round mic button that turns into Stop while [recording], with a ring
/// that follows the input [level], and a spinner while [busy].
class MicButton extends StatelessWidget {
  const MicButton({
    super.key,
    required this.recording,
    required this.busy,
    required this.level,
    required this.startLabel,
    required this.stopLabel,
    required this.onPressed,
  });

  static const _size = 88.0;
  static const _ring = 128.0;

  final bool recording;
  final bool busy;
  final ValueListenable<double> level;

  /// What a screen reader says the button does, idle and recording.
  final String startLabel;
  final String stopLabel;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return SizedBox.square(
      dimension: _ring,
      child: Stack(
        alignment: Alignment.center,
        children: [
          if (recording)
            ValueListenableBuilder(
              valueListenable: level,
              builder: (context, value, _) => AnimatedScale(
                scale: _size / _ring + (1 - _size / _ring) * value,
                duration: const Duration(milliseconds: 100),
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: scheme.error.withValues(alpha: 0.2),
                  ),
                  child: const SizedBox.square(dimension: _ring),
                ),
              ),
            ),
          Semantics(
            label: recording ? stopLabel : startLabel,
            // Excluding the button's own semantics drops its tap too.
            excludeSemantics: true,
            button: true,
            enabled: onPressed != null,
            onTap: onPressed,
            child: SizedBox.square(
              dimension: _size,
              child: FilledButton(
                onPressed: onPressed,
                style: FilledButton.styleFrom(
                  shape: const CircleBorder(),
                  padding: EdgeInsets.zero,
                  backgroundColor: recording ? scheme.error : null,
                  foregroundColor: recording ? scheme.onError : null,
                ),
                child: busy
                    ? const SizedBox.square(
                        dimension: 28,
                        child: CircularProgressIndicator(strokeWidth: 3),
                      )
                    : Icon(
                        recording ? Icons.stop_rounded : Icons.mic,
                        size: 40,
                      ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The recording's length (or Paused), what tapping the mic does now, and
/// Discard.
class RecordingCaption extends StatelessWidget {
  const RecordingCaption({
    super.key,
    required this.elapsed,
    required this.paused,
    required this.onDiscard,
    this.action = DictateStrings.tapToSend,
  });

  final ValueListenable<Duration> elapsed;
  final bool paused;
  final VoidCallback onDiscard;

  /// What tapping the mic again does.
  final String action;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Flexible(
          child: ValueListenableBuilder(
            valueListenable: elapsed,
            builder: (context, value, _) {
              final seconds = (value.inSeconds % 60).toString().padLeft(2, '0');
              final time = paused
                  ? DictateStrings.paused
                  : '${value.inMinutes}:$seconds';
              return Text(
                '$time · $action',
                style: mutedText(
                  context,
                )?.copyWith(fontFeatures: const [FontFeature.tabularFigures()]),
              );
            },
          ),
        ),
        const SizedBox(width: 8),
        TextButton(
          onPressed: onDiscard,
          child: const Text(DictateStrings.discard),
        ),
      ],
    );
  }
}
