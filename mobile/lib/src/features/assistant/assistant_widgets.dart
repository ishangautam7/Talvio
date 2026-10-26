// The parts of the assistant chat that the Ask page and live view's answer
// card share: a question with its answer (design spec §3.4), and the
// composer to ask from.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';

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

/// The answer's Markdown, in the app's type (§3.4).
MarkdownStyleSheet answerMarkdown(BuildContext context) {
  final t = context.talvio;
  final body = TalvioType.body.copyWith(color: t.ink);
  final heading = TalvioType.title2.copyWith(color: t.ink);
  const headingPad = EdgeInsets.only(top: 4, bottom: 0);
  return MarkdownStyleSheet(
    p: body,
    pPadding: EdgeInsets.zero,
    blockSpacing: 12,
    strong: const TextStyle(fontWeight: FontWeight.w600),
    em: const TextStyle(fontWeight: FontWeight.w500),
    del: const TextStyle(decoration: TextDecoration.lineThrough),
    h1: heading,
    h2: heading,
    h3: heading,
    h4: TalvioType.bodyStrong.copyWith(color: t.ink),
    h5: TalvioType.bodyStrong.copyWith(color: t.ink),
    h6: TalvioType.bodyStrong.copyWith(color: t.ink),
    h1Padding: headingPad,
    h2Padding: headingPad,
    h3Padding: headingPad,
    h4Padding: EdgeInsets.zero,
    h5Padding: EdgeInsets.zero,
    h6Padding: EdgeInsets.zero,
    listBullet: body.copyWith(color: t.muted),
    listIndent: 20,
    listBulletPadding: const EdgeInsets.only(right: 6),
    a: TextStyle(
      color: t.accent,
      decoration: TextDecoration.underline,
      decorationColor: t.accent,
    ),
    code: TalvioType.mono.copyWith(color: t.ink, backgroundColor: t.fill),
    codeblockPadding: const EdgeInsets.all(12),
    codeblockDecoration: BoxDecoration(
      color: t.fill,
      borderRadius: BorderRadius.circular(Radii.md),
    ),
    blockquote: body,
    blockquotePadding: const EdgeInsets.only(left: 12),
    blockquoteDecoration: BoxDecoration(
      border: BorderDirectional(start: BorderSide(color: t.border, width: 2)),
    ),
    tableHead: TalvioType.rowBody.copyWith(
      color: t.ink,
      fontWeight: FontWeight.w600,
    ),
    tableBody: TalvioType.rowBody.copyWith(color: t.ink),
    tableBorder: TableBorder.all(color: t.hairline),
    tableCellsPadding: const EdgeInsets.all(8),
    tableHeadCellsPadding: const EdgeInsets.all(8),
    horizontalRuleDecoration: BoxDecoration(
      border: Border(top: BorderSide(color: t.hairline)),
    ),
  );
}

/// One question and its answer. [compact] (live view) shows the question
/// as one muted line instead of a bubble.
class TurnView extends StatelessWidget {
  const TurnView({
    super.key,
    required this.turn,
    required this.controller,
    this.compact = false,
    this.showQuestion = true,
  });

  final AssistantTurn turn;
  final AssistantController controller;
  final bool compact;

  /// False when the question is shown above the turn already (live view's
  /// card header).
  final bool showQuestion;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final c = controller;
    final latest = identical(c.turns.lastOrNull, turn);
    final answer = turn.answer.trim();
    final streaming = !turn.done && answer.isNotEmpty;
    final approval = !turn.done && turn.stage == 'approval' && !c.cancelling;

    final reply = <Widget>[
      if (answer.isNotEmpty)
        Semantics(
          label: AssistantStrings.assistantOn(turn.computerName),
          child: MarkdownBody(
            data: answer,
            softLineBreak: true,
            selectable: !compact,
            styleSheet: answerMarkdown(context),
          ),
        ),
      if (streaming)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Align(
            alignment: AlignmentDirectional.centerStart,
            child: BlinkDot(),
          ),
        )
      else if (!turn.done)
        Semantics(
          liveRegion: true,
          child: AnimatedSwitcher(
            duration: Motion.quick,
            child: Align(
              key: ValueKey(turnStatus(turn, cancelling: c.cancelling)),
              alignment: AlignmentDirectional.centerStart,
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const TalvioOrb(size: 18, mood: OrbMood.thinking),
                  const SizedBox(width: Space.s12),
                  Flexible(
                    child: ShimmerText(
                      turnStatus(turn, cancelling: c.cancelling),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      if (approval && !compact)
        Padding(
          padding: const EdgeInsets.only(top: Space.s12),
          child: InlineBanner(
            tone: Tone.warning,
            icon: AppIcons.waiting,
            margin: EdgeInsets.zero,
            message: DictateStrings.approvalDetail,
            action: Btn(
              AssistantStrings.cancel,
              kind: BtnKind.ghostNeutral,
              compact: true,
              onPressed: c.cancelRequest,
            ),
          ),
        ),
      if (turn.error case final error?)
        Padding(
          padding: EdgeInsets.only(top: answer.isEmpty ? 0 : Space.s12),
          child: InlineBanner(
            tone: error == 'cancelled' ? Tone.neutral : Tone.danger,
            margin: EdgeInsets.zero,
            message: AssistantStrings.error(
              error,
              turn.computerName,
              message: turn.message,
            ),
            action: latest && c.canRetry && c.idle
                ? Btn(
                    AssistantStrings.tryAgain,
                    kind: BtnKind.ghost,
                    compact: true,
                    onPressed: () {
                      Haptic.tap();
                      unawaited(c.retry());
                    },
                  )
                : null,
          ),
        ),
      if (turn.shownActions.isNotEmpty)
        _Actions(turn.shownActions, computerName: turn.computerName),
      if (turn.pending != null)
        _PendingCard(turn: turn, controller: controller),
      if (turn.done && answer.isNotEmpty && !compact) _CopyButton(answer),
    ];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (turn.newConversation)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.s24),
            child: Row(
              children: [
                Expanded(child: Divider(color: t.hairline)),
                const SizedBox(width: Space.s12),
                Flexible(
                  flex: 4,
                  child: Text(
                    AssistantStrings.newConversation(turn.computerName),
                    textAlign: TextAlign.center,
                    style: TalvioType.footnote.copyWith(color: t.muted),
                  ),
                ),
                const SizedBox(width: Space.s12),
                Expanded(child: Divider(color: t.hairline)),
              ],
            ),
          ),
        if (!showQuestion)
          const SizedBox.shrink()
        else if (compact)
          Text(
            turn.question ?? AssistantStrings.voiceQuestion,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TalvioType.footnote.copyWith(color: t.muted),
          )
        else
          _QuestionBubble(turn: turn),
        if (showQuestion) SizedBox(height: compact ? Space.s8 : Space.s16),
        ...reply,
      ],
    );
  }
}

class _QuestionBubble extends StatelessWidget {
  const _QuestionBubble({required this.turn});

  final AssistantTurn turn;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final question = turn.question;
    return Align(
      alignment: AlignmentDirectional.centerEnd,
      child: Semantics(
        label: AssistantStrings.you,
        child: LayoutBuilder(
          builder: (context, constraints) => ConstrainedBox(
            constraints: BoxConstraints(maxWidth: constraints.maxWidth * 0.8),
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: t.fill,
                borderRadius: BorderRadius.circular(Radii.xl),
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: 14,
                  vertical: 10,
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    if (turn.spoken) ...[
                      Padding(
                        padding: const EdgeInsets.only(top: 4),
                        child: Icon(AppIcons.mic, size: 16, color: t.muted),
                      ),
                      const SizedBox(width: Space.s4),
                    ],
                    Flexible(
                      child: Text(
                        question ?? AssistantStrings.voiceQuestion,
                        style: TalvioType.body.copyWith(
                          color: question == null ? t.muted : t.ink,
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
    );
  }
}

/// What the assistant did on the computer: a small receipt under the
/// answer, one line per action.
class _Actions extends StatelessWidget {
  const _Actions(this.actions, {required this.computerName});

  final List<AssistantAction> actions;
  final String computerName;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.only(top: Space.s16),
      child: Container(
        padding: const EdgeInsetsDirectional.fromSTEB(14, 12, 14, 12),
        decoration: BoxDecoration(
          color: t.card,
          borderRadius: BorderRadius.circular(Radii.lg),
          boxShadow: Depth.card(t),
          border: Depth.edge(t),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              AssistantStrings.didOn(computerName),
              style: TalvioType.footnote.copyWith(color: t.muted),
            ),
            const SizedBox(height: Space.s8),
            for (final (i, action) in actions.indexed) ...[
              if (i > 0) const SizedBox(height: Space.s8),
              _ActionRow(action),
            ],
          ],
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
    final (IconData icon, Color color, Color wash) = switch (action.status) {
      'done' => (AppIcons.check, t.success, t.successSoft),
      'failed' => (AppIcons.close, t.danger, t.dangerSoft),
      'cancelled' => (AppIcons.cancelled, t.muted, t.fill),
      _ => (AppIcons.waiting, t.muted, t.fill),
    };
    return Semantics(
      label: AssistantStrings.actionLabel(action.label, action.status),
      excludeSemantics: true,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 22,
            height: 22,
            margin: const EdgeInsets.only(top: 1),
            decoration: BoxDecoration(color: wash, shape: BoxShape.circle),
            child: Icon(icon, size: 14, color: color),
          ),
          const SizedBox(width: Space.s12),
          Expanded(
            child: Text(
              action.label,
              style: TalvioType.rowTitle.copyWith(color: t.ink),
            ),
          ),
        ],
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
      'done' => (AssistantStrings.confirmedDone, AppIcons.done, t.success),
      'cancelled' => (
        AssistantStrings.confirmedCancelled,
        AppIcons.cancelledFill,
        t.muted,
      ),
      'needs_answer' => (
        AssistantStrings.confirmedAsks,
        AppIcons.questionFill,
        t.accent,
      ),
      _ => (AssistantStrings.confirmedFailed, AppIcons.failed, t.danger),
    };
    return Padding(
      padding: const EdgeInsets.only(top: Space.s12),
      child: Container(
        padding: const EdgeInsets.all(Space.s16),
        decoration: BoxDecoration(
          color: t.card,
          borderRadius: BorderRadius.circular(Radii.lg),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Icon(AppIcons.waiting, size: 16, color: t.muted),
                const SizedBox(width: 6),
                Text(
                  AssistantStrings.goAhead,
                  style: TalvioType.footnote.copyWith(color: t.muted),
                ),
              ],
            ),
            const SizedBox(height: Space.s4),
            Text(
              pending.summary,
              style: TalvioType.bodyStrong.copyWith(color: t.ink),
            ),
            if (turn.confirmFeedback case final feedback?
                when feedback.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: Space.s4),
                child: Semantics(
                  liveRegion: true,
                  child: Text(
                    feedback,
                    style: TalvioType.rowBody.copyWith(color: t.ink),
                  ),
                ),
              ),
            if (turn.confirmError case final error?)
              Padding(
                padding: const EdgeInsets.only(top: Space.s4),
                child: Semantics(
                  liveRegion: true,
                  child: Text(
                    // A confirmation is a command request: its errors are
                    // the command's.
                    CommandStrings.error(error, turn.computerName),
                    style: TalvioType.rowBody.copyWith(color: t.danger),
                  ),
                ),
              ),
            const SizedBox(height: Space.s16),
            AnimatedSwitcher(
              duration: Motion.quick,
              child: switch (ended) {
                (final label, final icon, final color) => Row(
                  key: const ValueKey('ended'),
                  children: [
                    Icon(icon, size: 16, color: color),
                    const SizedBox(width: Space.s8),
                    Expanded(
                      child: Semantics(
                        liveRegion: true,
                        child: Text(
                          label,
                          style: TalvioType.rowBody.copyWith(color: t.ink),
                        ),
                      ),
                    ),
                  ],
                ),
                null => Row(
                  key: const ValueKey('ask'),
                  children: [
                    Expanded(
                      child: Btn(
                        AssistantStrings.cancel,
                        kind: BtnKind.secondary,
                        height: 44,
                        onPressed: enabled && !turn.confirming
                            ? () {
                                Haptic.tick();
                                unawaited(c.confirm(turn, accept: false));
                              }
                            : null,
                      ),
                    ),
                    const SizedBox(width: Space.s8),
                    Expanded(
                      child: Btn(
                        AssistantStrings.doIt,
                        height: 44,
                        loading: turn.confirming,
                        onPressed: enabled
                            ? () {
                                Haptic.firm();
                                unawaited(c.confirm(turn, accept: true));
                              }
                            : null,
                      ),
                    ),
                  ],
                ),
              },
            ),
          ],
        ),
      ),
    );
  }
}

/// Copy, under a finished answer; a check for a moment once it is copied.
class _CopyButton extends StatefulWidget {
  const _CopyButton(this.text);

  final String text;

  @override
  State<_CopyButton> createState() => _CopyButtonState();
}

class _CopyButtonState extends State<_CopyButton> {
  var _copied = false;
  Timer? _reset;

  @override
  void dispose() {
    _reset?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    // The glyph lines up with the answer's text.
    return Padding(
      padding: const EdgeInsets.only(top: Space.s8),
      child: Align(
        alignment: AlignmentDirectional.centerStart,
        child: Transform.translate(
          offset: Offset(
            Directionality.of(context) == TextDirection.rtl ? 12 : -12,
            0,
          ),
          child: IconBtn(
            icon: _copied ? AppIcons.check : AppIcons.copy,
            tooltip: AssistantStrings.copy,
            color: t.muted,
            onPressed: () {
              Haptic.tick();
              unawaited(Clipboard.setData(ClipboardData(text: widget.text)));
              setState(() => _copied = true);
              _reset?.cancel();
              _reset = Timer(const Duration(milliseconds: 1500), () {
                if (mounted) setState(() => _copied = false);
              });
              showSnack(context, AssistantStrings.copied);
            },
          ),
        ),
      ),
    );
  }
}

/// The composer to ask from: type, or tap the mic and speak. While
/// recording it shows the waveform with Discard and Send.
class AssistantInput extends StatelessWidget {
  const AssistantInput({
    super.key,
    required this.controller,
    required this.enabled,
    this.dense = false,
    this.offlineName,
    this.margin = const EdgeInsets.fromLTRB(16, 8, 16, 8),
  });

  final AssistantController controller;

  /// The computer can be asked now.
  final bool enabled;

  /// Fewer lines, for live view.
  final bool dense;

  /// The computer's name while it is offline: the field says so.
  final String? offlineName;
  final EdgeInsetsGeometry margin;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final canAsk = enabled && c.idle;
    return ListenableBuilder(
      listenable: VoiceRecorder.holder,
      builder: (context, _) => Composer(
        hint: switch (offlineName) {
          final name? => AssistantStrings.offlinePlaceholder(name),
          null => AssistantStrings.typeHint,
        },
        sendLabel: AssistantStrings.send,
        enabled: canAsk,
        busy: c.phase == AssistantPhase.sending,
        maxLines: dense ? 2 : 6,
        margin: margin,
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
