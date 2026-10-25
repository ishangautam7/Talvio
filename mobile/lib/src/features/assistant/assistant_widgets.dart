// The parts of the assistant chat that the Assistant page and live view's
// assistant bar share: a question with its answer, and the input row.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';

import '../../design/brand.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../design/voice.dart';
import '../command/strings.dart';
import '../dictate/strings.dart';
import '../dictate/voice_recorder.dart';
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
/// question's bubble and the avatar, and shows the question as one muted
/// line instead.
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
    final t = context.talvio;
    final c = controller;
    final latest = identical(c.turns.lastOrNull, turn);
    final question = turn.question ?? AssistantStrings.voiceQuestion;
    final answer = turn.answer.trim();

    final reply = <Widget>[
      if (answer.isNotEmpty)
        Semantics(
          label: AssistantStrings.assistantOn(turn.computerName),
          child: MarkdownBody(
            data: answer,
            softLineBreak: true,
            selectable: !compact,
            styleSheet: _markdown(context),
          ),
        ),
      if (!turn.done)
        if (turn.stage == 'approval' && !c.cancelling && !compact)
          Padding(
            padding: const EdgeInsets.only(top: Space.xs),
            child: ApprovalCard(
              title: DictateStrings.approval(turn.computerName),
              body: DictateStrings.approvalDetail,
              onCancel: c.cancelRequest,
              cancelLabel: AssistantStrings.cancel,
            ),
          )
        else
          _Status(
            text: turnStatus(turn, cancelling: c.cancelling),
            tool: turn.stage == 'tool' && turn.toolLabel != null,
          ),
      if (turn.error case final error?)
        Padding(
          padding: const EdgeInsets.only(top: Space.xs),
          child: Notice(
            tone: error == 'cancelled' ? Tone.neutral : Tone.danger,
            message: AssistantStrings.error(
              error,
              turn.computerName,
              message: turn.message,
            ),
            actions: [
              if (latest && c.canRetry && c.idle)
                TextButton(
                  onPressed: () {
                    Haptic.tap();
                    unawaited(c.retry());
                  },
                  child: const Text(AssistantStrings.tryAgain),
                ),
            ],
          ),
        ),
      if (turn.shownActions.isNotEmpty) _Actions(turn.shownActions),
      if (turn.pending != null)
        _PendingCard(turn: turn, controller: controller),
      if (turn.done && answer.isNotEmpty && !compact) _CopyButton(answer),
    ];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (turn.newConversation)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.md),
            child: Row(
              children: [
                Expanded(child: Divider(color: t.hairline)),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: Space.sm),
                  child: Text(
                    AssistantStrings.newConversation(turn.computerName),
                    textAlign: TextAlign.center,
                    style: context.text.bodySmall,
                  ),
                ),
                Expanded(child: Divider(color: t.hairline)),
              ],
            ),
          ),
        if (compact)
          Text(
            question,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodyMedium?.copyWith(
              color: t.muted,
              fontStyle: FontStyle.italic,
            ),
          )
        else
          _QuestionBubble(turn: turn, question: question),
        const SizedBox(height: Space.md),
        if (compact)
          ...reply
        else
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Padding(
                padding: EdgeInsets.only(top: 2),
                child: TalvioMark(size: 26),
              ),
              const SizedBox(width: Space.md),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: reply,
                ),
              ),
            ],
          ),
      ],
    );
  }

  static MarkdownStyleSheet _markdown(BuildContext context) {
    final t = context.talvio;
    final text = context.text;
    return MarkdownStyleSheet.fromTheme(Theme.of(context)).copyWith(
      p: text.bodyLarge,
      h1: text.titleLarge,
      h2: text.titleLarge,
      h3: text.titleMedium,
      listBullet: text.bodyLarge,
      strong: const TextStyle(fontWeight: FontWeight.w700),
      a: TextStyle(color: t.accent, decoration: TextDecoration.underline),
      code: text.bodyMedium?.copyWith(
        fontFamily: 'monospace',
        backgroundColor: t.sunken,
      ),
      codeblockDecoration: BoxDecoration(
        color: t.sunken,
        borderRadius: BorderRadius.circular(Radii.sm),
      ),
      blockquoteDecoration: BoxDecoration(
        border: Border(left: BorderSide(color: t.hairline, width: 3)),
      ),
      blockSpacing: 10,
    );
  }
}

class _QuestionBubble extends StatelessWidget {
  const _QuestionBubble({required this.turn, required this.question});

  final AssistantTurn turn;
  final String question;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Align(
      alignment: AlignmentDirectional.centerEnd,
      child: Semantics(
        label: AssistantStrings.you,
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxWidth: MediaQuery.sizeOf(context).width * 0.8,
          ),
          child: DecoratedBox(
            decoration: BoxDecoration(
              color: t.userBubble,
              borderRadius: const BorderRadiusDirectional.only(
                topStart: Radius.circular(20),
                topEnd: Radius.circular(20),
                bottomStart: Radius.circular(20),
                bottomEnd: Radius.circular(6),
              ),
            ),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (turn.spoken) ...[
                    Icon(
                      Icons.graphic_eq_rounded,
                      size: 16,
                      color: t.onUserBubble.withValues(alpha: 0.7),
                    ),
                    const SizedBox(width: 6),
                  ],
                  Flexible(
                    child: Text(
                      question,
                      style: context.text.bodyLarge?.copyWith(
                        color: t.onUserBubble,
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
    );
  }
}

/// What the computer is doing with the question. Stopping it is the
/// composer's stop button.
class _Status extends StatelessWidget {
  const _Status({required this.text, required this.tool});

  final String text;

  /// A tool is at work: shown as a chip.
  final bool tool;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.only(top: 3),
      child: Row(
        children: [
          if (tool)
            Flexible(
              child: Semantics(
                liveRegion: true,
                child: Pill(
                  text,
                  tone: Tone.accent,
                  icon: Icons.handyman_outlined,
                ),
              ),
            )
          else ...[
            const WorkingDots(),
            const SizedBox(width: Space.md),
            Flexible(
              child: Semantics(
                liveRegion: true,
                child: Text(
                  text,
                  style: context.text.bodyMedium?.copyWith(color: t.muted),
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// What the assistant did on the computer.
class _Actions extends StatelessWidget {
  const _Actions(this.actions);

  final List<AssistantAction> actions;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.only(top: Space.md),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: t.sunken,
          borderRadius: BorderRadius.circular(Radii.md),
        ),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(
            Space.md,
            Space.sm,
            Space.md,
            Space.sm,
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                AssistantStrings.didOnComputer,
                style: context.text.labelSmall?.copyWith(color: t.faint),
              ),
              const SizedBox(height: Space.xs),
              for (final action in actions) _ActionRow(action),
            ],
          ),
        ),
      ),
    );
  }
}

class _ActionRow extends StatelessWidget {
  const _ActionRow(this.action);

  final AssistantAction action;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final (IconData icon, Color color) = switch (action.status) {
      'done' => (Icons.check_circle_rounded, t.success),
      'failed' => (Icons.error_rounded, t.danger),
      'cancelled' => (Icons.block_rounded, t.muted),
      _ => (Icons.schedule_rounded, t.muted),
    };
    return Semantics(
      label: AssistantStrings.actionLabel(action.label, action.status),
      excludeSemantics: true,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 3),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(icon, size: 18, color: color),
            const SizedBox(width: Space.sm),
            Expanded(child: Text(action.label, style: context.text.bodyMedium)),
          ],
        ),
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
    final t = context.talvio;
    final pending = turn.pending!;
    final c = controller;
    final enabled = turn.awaitingConfirmation && c.idle;
    final ended = switch (turn.confirmStatus) {
      null => null,
      'done' => AssistantStrings.confirmedDone,
      'cancelled' => AssistantStrings.confirmedCancelled,
      'needs_answer' => AssistantStrings.confirmedAsks,
      _ => AssistantStrings.confirmedFailed,
    };
    final (Tone tone, IconData icon) = switch (turn.confirmStatus) {
      null => (Tone.accent, Icons.front_hand_outlined),
      'done' => (Tone.success, Icons.check_rounded),
      'cancelled' => (Tone.neutral, Icons.block_rounded),
      'needs_answer' => (Tone.accent, Icons.help_outline_rounded),
      _ => (Tone.danger, Icons.error_outline_rounded),
    };
    return Padding(
      padding: const EdgeInsets.only(top: Space.md),
      child: Surface(
        borderColor: ended == null ? t.accent.withValues(alpha: 0.5) : null,
        padding: const EdgeInsets.fromLTRB(
          Space.lg,
          Space.md,
          Space.lg,
          Space.md,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                IconTile(icon, tone: tone, size: 28),
                const SizedBox(width: Space.sm),
                Expanded(
                  child: Text(
                    ended ?? AssistantStrings.goAhead,
                    style: context.text.labelLarge?.copyWith(color: t.muted),
                  ),
                ),
              ],
            ),
            const SizedBox(height: Space.sm),
            Text(pending.summary, style: context.text.titleMedium),
            if (turn.confirmFeedback case final feedback?
                when feedback.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: Space.xs),
                child: Semantics(
                  liveRegion: true,
                  child: Text(feedback, style: context.text.bodyMedium),
                ),
              ),
            if (turn.confirmError case final error?)
              Padding(
                padding: const EdgeInsets.only(top: Space.xs),
                child: Semantics(
                  liveRegion: true,
                  child: Text(
                    // A confirmation is a command request: its errors are
                    // the command's.
                    CommandStrings.error(error, turn.computerName),
                    style: context.text.bodyMedium?.copyWith(color: t.danger),
                  ),
                ),
              ),
            if (ended == null)
              Padding(
                padding: const EdgeInsets.only(top: Space.md),
                child: Row(
                  children: [
                    Expanded(
                      child: OutlinedButton(
                        onPressed: enabled
                            ? () {
                                Haptic.tick();
                                unawaited(c.confirm(turn, accept: false));
                              }
                            : null,
                        child: const Text(AssistantStrings.cancel),
                      ),
                    ),
                    const SizedBox(width: Space.sm),
                    Expanded(
                      child: FilledButton(
                        onPressed: enabled
                            ? () {
                                Haptic.firm();
                                unawaited(c.confirm(turn, accept: true));
                              }
                            : null,
                        child: turn.confirming
                            ? const Text(AssistantStrings.confirming)
                            : const Text(AssistantStrings.doIt),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _CopyButton extends StatelessWidget {
  const _CopyButton(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Align(
    alignment: AlignmentDirectional.centerStart,
    child: IconButton(
      tooltip: AssistantStrings.copy,
      visualDensity: VisualDensity.compact,
      iconSize: 18,
      color: context.talvio.muted,
      icon: const Icon(Icons.copy_rounded),
      onPressed: () {
        Haptic.tick();
        unawaited(Clipboard.setData(ClipboardData(text: text)));
        ScaffoldMessenger.maybeOf(context)
          ?..hideCurrentSnackBar()
          ..showSnackBar(
            const SnackBar(content: Text(AssistantStrings.copied)),
          );
      },
    ),
  );
}

/// The bar to ask from: type, or tap the mic and speak. While recording it
/// shows the waveform with Discard and Send.
class AssistantInput extends StatelessWidget {
  const AssistantInput({
    super.key,
    required this.controller,
    required this.enabled,
    this.dense = false,
  });

  final AssistantController controller;

  /// The computer can be asked now.
  final bool enabled;

  /// Fewer lines, for live view.
  final bool dense;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final canAsk = enabled && c.idle;
    return ListenableBuilder(
      listenable: VoiceRecorder.holder,
      builder: (context, _) => Composer(
        hint: AssistantStrings.typeHint,
        sendLabel: AssistantStrings.send,
        enabled: canAsk,
        busy: c.phase == AssistantPhase.sending,
        maxLines: dense ? 2 : 5,
        inputFormatters: [_questionLimit],
        onSubmit: c.ask,
        onCancel: c.cancelling ? null : c.cancelRequest,
        cancelLabel: AssistantStrings.stopAnswer,
        voice: VoiceState(
          recording: c.phase == AssistantPhase.recording,
          paused: c.paused,
          level: c.level,
          elapsed: c.elapsed,
          onStart: canAsk && !c.micBlocked ? c.startRecording : null,
          onStop: c.stopAndSend,
          onDiscard: c.discardRecording,
          startLabel: AssistantStrings.startRecording,
          stopLabel: AssistantStrings.stopAndSend,
          discardLabel: AssistantStrings.discard,
          pausedLabel: DictateStrings.paused,
        ),
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
