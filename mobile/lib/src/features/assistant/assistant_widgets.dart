// The parts of the assistant chat that the Assistant page and live view's
// assistant bar share: a question with its answer, and the input row.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';

import '../dictate/computer_widgets.dart' show mutedText;
import '../dictate/strings.dart';
import 'assistant_controller.dart';
import 'strings.dart';

/// The status line of [turn] while it is answered.
String turnStatus(AssistantTurn turn, {required bool cancelling}) {
  final name = turn.computerName;
  if (cancelling) return AssistantStrings.cancelling;
  return switch (turn.stage) {
    'approval' => DictateStrings.approval(name),
    'transcribing' => AssistantStrings.transcribing(name),
    'thinking' => AssistantStrings.thinking,
    'tool' => switch (turn.toolLabel) {
      final label? => AssistantStrings.tool(label),
      null => AssistantStrings.thinking,
    },
    'answer' => AssistantStrings.answering,
    _ => AssistantStrings.sending(name),
  };
}

/// One question and its answer. [compact] (live view) leaves out the
/// question's bubble and shows it as one muted line instead.
class TurnView extends StatelessWidget {
  const TurnView({
    super.key,
    required this.turn,
    required this.controller,
    this.compact = false,
  });

  final AssistantTurn turn;
  final AssistantController controller;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final c = controller;
    final latest = identical(c.turns.lastOrNull, turn);
    final question = turn.question ?? AssistantStrings.voiceQuestion;
    final answer = turn.answer.trim();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (turn.newConversation)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Text(
              AssistantStrings.newConversation(turn.computerName),
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(
                color: scheme.onSurfaceVariant,
              ),
            ),
          ),
        if (compact)
          Text(
            question,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: mutedText(context)?.copyWith(fontStyle: FontStyle.italic),
          )
        else
          Align(
            alignment: AlignmentDirectional.centerEnd,
            child: Semantics(
              label: AssistantStrings.you,
              child: ConstrainedBox(
                constraints: BoxConstraints(
                  maxWidth: MediaQuery.sizeOf(context).width * 0.8,
                ),
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: scheme.primaryContainer,
                    borderRadius: BorderRadius.circular(18),
                  ),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 14,
                      vertical: 10,
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (turn.spoken) ...[
                          Icon(
                            Icons.mic_none,
                            size: 16,
                            color: scheme.onPrimaryContainer,
                          ),
                          const SizedBox(width: 6),
                        ],
                        Flexible(
                          child: Text(
                            question,
                            style: theme.textTheme.bodyLarge?.copyWith(
                              color: scheme.onPrimaryContainer,
                              fontStyle: turn.question == null
                                  ? FontStyle.italic
                                  : null,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        const SizedBox(height: 8),
        if (answer.isNotEmpty)
          Semantics(
            label: AssistantStrings.assistantOn(turn.computerName),
            child: MarkdownBody(
              data: answer,
              softLineBreak: true,
              styleSheet: MarkdownStyleSheet.fromTheme(theme).copyWith(
                p: theme.textTheme.bodyLarge,
                code: theme.textTheme.bodyMedium?.copyWith(
                  fontFamily: 'monospace',
                  backgroundColor: scheme.surfaceContainerHighest,
                ),
              ),
            ),
          ),
        if (!turn.done)
          _Status(
            text: turnStatus(turn, cancelling: c.cancelling),
            onCancel: c.cancelling ? null : c.cancelRequest,
          ),
        if (turn.error case final error?)
          _ErrorBox(
            text: AssistantStrings.error(
              error,
              turn.computerName,
              message: turn.message,
            ),
            onRetry: latest && c.canRetry && c.idle ? c.retry : null,
          ),
        if (turn.actions.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Wrap(
              spacing: 6,
              runSpacing: 6,
              children: [
                for (final action in turn.actions) _ActionChip(action),
              ],
            ),
          ),
        if (turn.pending != null)
          _PendingCard(turn: turn, controller: controller),
        if (turn.done && answer.isNotEmpty && !compact)
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: _CopyButton(answer),
          ),
      ],
    );
  }
}

class _Status extends StatelessWidget {
  const _Status({required this.text, required this.onCancel});

  final String text;
  final VoidCallback? onCancel;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Row(
        children: [
          const SizedBox.square(
            dimension: 16,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Semantics(
              liveRegion: true,
              child: Text(text, style: mutedText(context)),
            ),
          ),
          TextButton(
            onPressed: onCancel,
            child: const Text(AssistantStrings.cancel),
          ),
        ],
      ),
    );
  }
}

class _ErrorBox extends StatelessWidget {
  const _ErrorBox({required this.text, required this.onRetry});

  final String text;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: scheme.errorContainer,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 10, 4, 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(Icons.error_outline, size: 20, color: scheme.error),
                const SizedBox(width: 8),
                Expanded(
                  child: Semantics(
                    liveRegion: true,
                    child: Text(
                      text,
                      style: theme.textTheme.bodyMedium?.copyWith(
                        color: scheme.onErrorContainer,
                      ),
                    ),
                  ),
                ),
              ],
            ),
            if (onRetry != null)
              OverflowBar(
                alignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: onRetry,
                    child: const Text(AssistantStrings.tryAgain),
                  ),
                ],
              )
            else
              const SizedBox(height: 6),
          ],
        ),
      ),
    );
  }
}

class _ActionChip extends StatelessWidget {
  const _ActionChip(this.action);

  final AssistantAction action;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (IconData icon, Color color) = switch (action.status) {
      'done' => (Icons.check_circle, scheme.primary),
      'failed' => (Icons.error_outline, scheme.error),
      'cancelled' => (Icons.block, scheme.onSurfaceVariant),
      _ => (Icons.schedule, scheme.onSurfaceVariant),
    };
    return Semantics(
      label: AssistantStrings.actionLabel(action.label, action.status),
      excludeSemantics: true,
      child: Chip(
        visualDensity: VisualDensity.compact,
        avatar: Icon(icon, size: 18, color: color),
        label: Text(action.label),
      ),
    );
  }
}

/// What the assistant wants to do and is waiting for: Do it or Cancel, then
/// how it went.
class _PendingCard extends StatelessWidget {
  const _PendingCard({required this.turn, required this.controller});

  final AssistantTurn turn;
  final AssistantController controller;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final pending = turn.pending!;
    final c = controller;
    final enabled = turn.awaitingConfirmation && c.idle;
    final ended = switch (turn.confirmStatus) {
      null => null,
      'done' => AssistantStrings.confirmedDone,
      'cancelled' => AssistantStrings.confirmedCancelled,
      _ => AssistantStrings.confirmedFailed,
    };
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Card.filled(
        margin: EdgeInsets.zero,
        color: scheme.secondaryContainer,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 10, 8, 4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                ended ?? AssistantStrings.goAhead,
                style: theme.textTheme.labelLarge?.copyWith(
                  color: scheme.onSecondaryContainer,
                ),
              ),
              const SizedBox(height: 2),
              Text(
                pending.summary,
                style: theme.textTheme.titleSmall?.copyWith(
                  color: scheme.onSecondaryContainer,
                ),
              ),
              if (turn.confirmFeedback case final feedback?
                  when feedback.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Semantics(liveRegion: true, child: Text(feedback)),
                ),
              if (turn.confirmError case final error?)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Semantics(
                    liveRegion: true,
                    child: Text(
                      AssistantStrings.error(error, turn.computerName),
                      style: TextStyle(color: scheme.error),
                    ),
                  ),
                ),
              if (ended == null)
                OverflowBar(
                  alignment: MainAxisAlignment.end,
                  spacing: 8,
                  children: [
                    TextButton(
                      onPressed: enabled
                          ? () => c.confirm(turn, accept: false)
                          : null,
                      child: const Text(AssistantStrings.cancel),
                    ),
                    FilledButton(
                      onPressed: enabled
                          ? () => c.confirm(turn, accept: true)
                          : null,
                      child: turn.confirming
                          ? const Text(AssistantStrings.confirming)
                          : const Text(AssistantStrings.doIt),
                    ),
                  ],
                )
              else
                const SizedBox(height: 8),
            ],
          ),
        ),
      ),
    );
  }
}

class _CopyButton extends StatelessWidget {
  const _CopyButton(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: AssistantStrings.copy,
    visualDensity: VisualDensity.compact,
    iconSize: 18,
    icon: const Icon(Icons.copy_outlined),
    onPressed: () {
      unawaited(Clipboard.setData(ClipboardData(text: text)));
      ScaffoldMessenger.maybeOf(context)
        ?..hideCurrentSnackBar()
        ..showSnackBar(const SnackBar(content: Text(AssistantStrings.copied)));
    },
  );
}

/// The row to ask from: the mic, the text field and Send. While recording,
/// the field gives way to the recording's length and Discard.
class AssistantInput extends StatefulWidget {
  const AssistantInput({
    super.key,
    required this.controller,
    required this.enabled,
    this.leading,
    this.dense = false,
  });

  final AssistantController controller;

  /// The computer can be asked now.
  final bool enabled;

  /// Before the mic (live view's Keys).
  final Widget? leading;

  /// Smaller, for live view.
  final bool dense;

  @override
  State<AssistantInput> createState() => _AssistantInputState();
}

class _AssistantInputState extends State<AssistantInput> {
  final _text = TextEditingController();
  final _focus = FocusNode();

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  Future<void> _send() async {
    final text = _text.text;
    if (text.trim().isEmpty) return;
    unawaited(HapticFeedback.lightImpact());
    if (await widget.controller.ask(text) && mounted) _text.clear();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final scheme = Theme.of(context).colorScheme;
    final recording = c.phase == AssistantPhase.recording;
    final canAsk = widget.enabled && c.idle;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        ?widget.leading,
        Semantics(
          label: recording
              ? AssistantStrings.stopAndSend
              : AssistantStrings.startRecording,
          button: true,
          excludeSemantics: true,
          enabled: recording || canAsk,
          onTap: recording ? c.stopAndSend : (canAsk ? c.startRecording : null),
          child: IconButton.filledTonal(
            tooltip: recording
                ? AssistantStrings.stopAndSend
                : AssistantStrings.startRecording,
            style: recording
                ? IconButton.styleFrom(
                    backgroundColor: scheme.error,
                    foregroundColor: scheme.onError,
                  )
                : null,
            onPressed: recording
                ? c.stopAndSend
                : (canAsk ? c.startRecording : null),
            icon: Icon(recording ? Icons.stop_rounded : Icons.mic),
          ),
        ),
        const SizedBox(width: 4),
        Expanded(
          child: recording
              ? _Recording(
                  elapsed: c.elapsed,
                  paused: c.paused,
                  onDiscard: c.discardRecording,
                )
              : TextField(
                  controller: _text,
                  focusNode: _focus,
                  enabled: canAsk,
                  minLines: 1,
                  maxLines: widget.dense ? 2 : 4,
                  inputFormatters: [_questionLimit],
                  textCapitalization: TextCapitalization.sentences,
                  textInputAction: TextInputAction.send,
                  onSubmitted: (_) => _send(),
                  onTapOutside: (_) => _focus.unfocus(),
                  decoration: InputDecoration(
                    hintText: AssistantStrings.typeHint,
                    isDense: widget.dense,
                  ),
                ),
        ),
        const SizedBox(width: 4),
        ValueListenableBuilder(
          valueListenable: _text,
          builder: (context, value, _) {
            final ready = canAsk && !recording && value.text.trim().isNotEmpty;
            return IconButton.filled(
              tooltip: AssistantStrings.send,
              onPressed: ready ? _send : null,
              icon: const Icon(Icons.arrow_upward),
            );
          },
        ),
      ],
    );
  }
}

class _Recording extends StatelessWidget {
  const _Recording({
    required this.elapsed,
    required this.paused,
    required this.onDiscard,
  });

  final ValueListenable<Duration> elapsed;
  final bool paused;
  final VoidCallback onDiscard;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 48,
      child: Row(
        children: [
          Expanded(
            child: ValueListenableBuilder(
              valueListenable: elapsed,
              builder: (context, value, _) {
                final seconds = (value.inSeconds % 60).toString().padLeft(
                  2,
                  '0',
                );
                return Text(
                  paused
                      ? DictateStrings.paused
                      : '${value.inMinutes}:$seconds',
                  style: mutedText(context)?.copyWith(
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                );
              },
            ),
          ),
          TextButton(
            onPressed: onDiscard,
            child: const Text(AssistantStrings.discard),
          ),
        ],
      ),
    );
  }
}

final _questionLimit = TextInputFormatter.withFunction((old, next) {
  if (next.text.runes.length <= maxQuestionChars) return next;
  final text = String.fromCharCodes(next.text.runes.take(maxQuestionChars));
  return TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
  );
});
