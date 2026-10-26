import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../design/voice.dart';
import '../../shell/feature_tab.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_scope.dart';
import '../dictate/strings.dart';
import '../dictate/voice_recorder.dart';
import 'command_controller.dart';
import 'command_scope.dart';
import 'recent_commands.dart';
import 'strings.dart';

/// Telling the computer what to do: "open YouTube", "play a song".
final doTab = FeatureTab(
  id: 'do',
  label: CommandStrings.tab,
  icon: AppIcons.doTab,
  selectedIcon: AppIcons.doSelected,
  requires: CommandKind.capability,
  builder: (_) => const CommandScreen(),
  composer: true,
);

/// An icon for each of [CommandStrings.exampleCommands].
const _exampleIcons = [
  AppIcons.youtube,
  AppIcons.music,
  AppIcons.pause,
  AppIcons.volume,
  AppIcons.mail,
];

class CommandScreen extends StatefulWidget {
  const CommandScreen({super.key});

  @override
  State<CommandScreen> createState() => _CommandScreenState();
}

class _CommandScreenState extends State<CommandScreen> {
  late final AppServices _services = AppServices.of(context);
  late final CommandController _controller = CommandScope.of(context);
  final _answer = TextEditingController();
  final _text = TextEditingController();
  final _focus = FocusNode();
  CommandOutcome? _seen;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_onOutcome);
  }

  @override
  void dispose() {
    _controller.removeListener(_onOutcome);
    _answer.dispose();
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  /// A buzz when a command ends in an error.
  void _onOutcome() {
    final outcome = _controller.outcome;
    if (identical(outcome, _seen)) return;
    _seen = outcome;
    final error = outcome?.error;
    final failed = outcome?.result?.status == CommandStatus.failed;
    if ((error != null && error != 'cancelled') || failed) Haptic.error();
  }

  Future<bool> _send(String text) async {
    if (text.trim().isEmpty) return false;
    FocusManager.instance.primaryFocus?.unfocus();
    Haptic.tap();
    return _controller.sendText(text);
  }

  Future<void> _sendAnswer(String text) async {
    if (text.trim().isEmpty) return;
    FocusManager.instance.primaryFocus?.unfocus();
    Haptic.tap();
    await _controller.answer(text);
    if (mounted) _answer.clear();
  }

  @override
  Widget build(BuildContext context) {
    final relay = _services.relay;
    return ListenableBuilder(
      listenable: Listenable.merge([
        _controller,
        relay.devices,
        relay.status,
        VoiceRecorder.holder,
        _focus,
      ]),
      builder: (context, _) => ComputerGate(
        controller: DictateScope.of(context),
        builder: _buildBody,
      ),
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool connected) {
    final c = _controller;
    final t = context.talvio;
    final idle = c.idle;
    final ready =
        connected &&
        computer.online &&
        computer.capabilities.contains(CommandKind.capability);
    final outcome = c.outcome;
    final name = computer.name;
    final sending = c.phase == CommandPhase.sending;
    final outdated =
        computer.online &&
        connected &&
        computer.capabilities.isNotEmpty &&
        !computer.capabilities.contains(CommandKind.capability);
    final recents = c.recents.items;

    final voice = VoiceState(
      recording: c.phase == CommandPhase.recording,
      paused: c.paused,
      level: c.level,
      elapsed: c.elapsed,
      onStart: ready && idle && !c.micBlocked ? c.startRecording : null,
      onStop: c.stopAndSend,
      onDiscard: c.discardRecording,
      startLabel: CommandStrings.startRecording,
      stopLabel: CommandStrings.stopAndSend,
      discardLabel: DictateStrings.discard,
      pausedLabel: DictateStrings.paused,
    );

    final composer = Composer(
      controller: _text,
      focusNode: _focus,
      hint: computer.online
          ? CommandStrings.typeHint
          : DictateStrings.offlineTitle(name),
      sendLabel: CommandStrings.sendCommand,
      voice: voice,
      enabled: ready && idle,
      busy: sending,
      onCancel: c.cancelling ? null : c.cancelRequest,
      cancelLabel: CommandStrings.cancel,
      maxLines: 4,
      // The field clears at once: the command shows in its card, and Try
      // again sends it again.
      onSubmit: (text) {
        unawaited(_send(text));
        return Future.value(true);
      },
    );

    // Offline or outdated with nothing to read: one empty state.
    if (recents.isEmpty && outcome == null && !sending) {
      if (!computer.online) {
        return Column(
          children: [
            Expanded(
              child: EmptyState(
                icon: DuoIcons.offline,
                title: DictateStrings.offlineTitle(name),
                body: CommandStrings.offlineNote(name),
              ),
            ),
            composer,
          ],
        );
      }
      if (outdated) {
        return Column(
          children: [
            Expanded(
              child: EmptyState(
                icon: DuoIcons.update,
                title: CommandStrings.updateTitle(name),
                body: CommandStrings.updateNote(name),
              ),
            ),
            composer,
          ],
        );
      }
    }

    final note = !computer.online
        ? CommandStrings.offlineNote(name)
        : outdated
        ? CommandStrings.updateNote(name)
        : null;

    final Widget? status = sending
        ? _Sending(controller: c, name: name)
        : outcome != null
        ? Appear(
            key: ObjectKey(outcome),
            child: _ResultCard(
              outcome: outcome,
              computerName: outcome.computer.isEmpty ? name : outcome.computer,
              enabled: ready && idle,
              answer: _answer,
              onConfirm: (accept) {
                accept ? Haptic.firm() : Haptic.tick();
                unawaited(c.confirm(accept: accept));
              },
              onAnswer: _sendAnswer,
              onRetry: c.canRetry && ready ? c.retry : null,
              onDismiss: c.dismissOutcome,
            ),
          )
        : null;

    final showRail =
        ready &&
        idle &&
        c.phase == CommandPhase.idle &&
        !_focus.hasFocus &&
        _text.text.isEmpty;

    final Widget content;
    if (recents.isEmpty && status == null && note == null) {
      content = Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                CommandStrings.prompt(name),
                textAlign: TextAlign.center,
                style: TalvioType.title1.copyWith(color: t.ink),
              ),
              const SizedBox(height: Space.s8),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 300),
                child: Text(
                  CommandStrings.promptDetail,
                  textAlign: TextAlign.center,
                  style: TalvioType.rowBody.copyWith(color: t.muted),
                ),
              ),
            ],
          ),
        ),
      );
    } else {
      content = ListView(
        padding: const EdgeInsets.only(bottom: Space.s16),
        children: [
          if (note != null && !sending) ...[
            const SizedBox(height: Space.s16),
            InlineBanner(
              icon: computer.online ? AppIcons.update : AppIcons.offline,
              message: note,
            ),
          ],
          if (status != null) ...[const SizedBox(height: Space.s16), status],
          if (recents.isNotEmpty) ...[
            const SectionLabel(CommandStrings.recent),
            Group(
              children: [
                for (final item in recents)
                  _RecentRow(
                    item,
                    onRunAgain: ready && idle
                        ? () => unawaited(_send(item.said))
                        : null,
                  ),
              ],
            ),
            const GroupFooter(CommandStrings.recentNote),
          ],
        ],
      );
    }

    return Column(
      children: [
        Expanded(child: content),
        AnimatedSwitcher(
          duration: Motion.quick,
          transitionBuilder: (child, animation) => SizeTransition(
            sizeFactor: animation,
            alignment: Alignment.bottomCenter,
            child: FadeTransition(opacity: animation, child: child),
          ),
          child: showRail
              ? _Examples(
                  key: const ValueKey('rail'),
                  onTap: (text) => unawaited(_send(text)),
                )
              : const SizedBox(width: double.infinity),
        ),
        composer,
      ],
    );
  }
}

/// A command on its way: approval, transcribing, at work.
class _Sending extends StatelessWidget {
  const _Sending({required this.controller, required this.name});

  final CommandController controller;
  final String name;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    if (c.stage == 'approval' && !c.cancelling) {
      return RequestCard(
        approval: true,
        title: DictateStrings.approval(name),
        detail: DictateStrings.approvalDetail,
        onCancel: c.cancelRequest,
        cancelLabel: CommandStrings.cancel,
      );
    }
    final text = c.cancelling
        ? CommandStrings.cancelling
        : switch (c.stage) {
            'transcribing' => CommandStrings.transcribing(name),
            'working' => CommandStrings.working(name),
            _ => CommandStrings.sending(name),
          };
    return RequestCard(
      title: text,
      detail: c.stage == 'working' && !c.cancelling
          ? CommandStrings.workingDetail
          : null,
      onCancel: c.cancelling ? null : c.cancelRequest,
      cancelLabel: CommandStrings.cancel,
    );
  }
}

/// The example commands, in one row above the composer.
class _Examples extends StatelessWidget {
  const _Examples({super.key, required this.onTap});

  final ValueChanged<String> onTap;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: Space.s8),
      child: SizedBox(
        height: 44,
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: Space.gutter),
          itemCount: CommandStrings.exampleCommands.length,
          separatorBuilder: (_, _) => const SizedBox(width: Space.s8),
          itemBuilder: (context, i) {
            final example = CommandStrings.exampleCommands[i];
            return Center(
              child: TalvioChip(
                label: example,
                icon: _exampleIcons[i % _exampleIcons.length],
                onPressed: () => onTap(example),
              ),
            );
          },
        ),
      ),
    );
  }
}

class _ResultCard extends StatelessWidget {
  const _ResultCard({
    required this.outcome,
    required this.computerName,
    required this.enabled,
    required this.answer,
    required this.onConfirm,
    required this.onAnswer,
    required this.onRetry,
    required this.onDismiss,
  });

  final CommandOutcome outcome;
  final String computerName;

  /// Do it, Cancel and answering can be sent now.
  final bool enabled;
  final TextEditingController answer;
  final ValueChanged<bool> onConfirm;
  final ValueChanged<String> onAnswer;
  final VoidCallback? onRetry;
  final VoidCallback onDismiss;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final result = outcome.result;
    final error = outcome.error;

    final (IconData icon, Color color, String title) = switch (result?.status) {
      _ when error == 'cancelled' => (
        AppIcons.cancelledFill,
        t.muted,
        CommandStrings.cancelled,
      ),
      _ when error != null => (
        AppIcons.failed,
        t.danger,
        CommandStrings.failed,
      ),
      CommandStatus.awaitingConfirmation => (
        AppIcons.waitingFill,
        t.accent,
        CommandStrings.awaitingConfirmation,
      ),
      CommandStatus.needsAnswer => (
        AppIcons.questionFill,
        t.accent,
        CommandStrings.needsAnswer,
      ),
      CommandStatus.cancelled => (
        AppIcons.cancelledFill,
        t.muted,
        CommandStrings.cancelled,
      ),
      CommandStatus.failed => (
        AppIcons.failed,
        t.danger,
        CommandStrings.failed,
      ),
      CommandStatus.needsComputerUse => (
        AppIcons.needsScreenFill,
        t.warning,
        CommandStrings.needsComputerUse,
      ),
      _ => (AppIcons.done, t.success, CommandStrings.done),
    };
    final waiting =
        error == null &&
        result != null &&
        (result.confirmable || result.answerable);

    final message = error != null
        ? CommandStrings.error(error, computerName)
        : result == null
        ? null
        : result.feedback.isNotEmpty
        ? result.feedback
        : result.status == CommandStatus.needsComputerUse
        ? CommandStrings.needsComputerUseBody
        : null;

    final card = Container(
      margin: const EdgeInsets.symmetric(horizontal: Space.gutter),
      padding: const EdgeInsets.all(Space.s16),
      decoration: BoxDecoration(
        color: t.card,
        borderRadius: BorderRadius.circular(Radii.lg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(
            height: 20,
            child: Row(
              children: [
                Icon(icon, size: 20, color: color),
                const SizedBox(width: Space.s8),
                Expanded(
                  child: Semantics(
                    liveRegion: true,
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TalvioType.label.copyWith(
                        color: color,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ),
                if (!waiting) const SizedBox(width: 32),
              ],
            ),
          ),
          if (outcome.said case final said?) ...[
            const SizedBox(height: Space.s8),
            _Said(said),
          ],
          if (message case final message?) ...[
            const SizedBox(height: Space.s4),
            Text(message, style: TalvioType.rowBody.copyWith(color: t.ink)),
          ],
          if (error == null && result != null && result.confirmable) ...[
            if (result.pending case final pending?) ...[
              const SizedBox(height: Space.s12),
              Container(
                padding: const EdgeInsets.all(Space.s12),
                decoration: BoxDecoration(
                  color: t.fill,
                  borderRadius: BorderRadius.circular(Radii.md),
                ),
                child: Row(
                  children: [
                    Icon(AppIcons.arrowRight, size: 16, color: t.muted),
                    const SizedBox(width: Space.s8),
                    Expanded(
                      child: Text(
                        pending,
                        style: TalvioType.rowTitle.copyWith(color: t.ink),
                      ),
                    ),
                  ],
                ),
              ),
            ],
            const SizedBox(height: Space.s16),
            Row(
              children: [
                Expanded(
                  child: Btn(
                    CommandStrings.cancel,
                    kind: BtnKind.secondary,
                    height: 44,
                    onPressed: enabled ? () => onConfirm(false) : null,
                  ),
                ),
                const SizedBox(width: Space.s8),
                Expanded(
                  child: Btn(
                    CommandStrings.doIt,
                    height: 44,
                    onPressed: enabled ? () => onConfirm(true) : null,
                  ),
                ),
              ],
            ),
          ],
          if (error == null && result != null && result.answerable) ...[
            if (result.question case final question?) ...[
              const SizedBox(height: Space.s12),
              Text(question, style: TalvioType.rowTitle.copyWith(color: t.ink)),
            ],
            if (result.choices.isNotEmpty) ...[
              const SizedBox(height: Space.s8),
              Wrap(
                spacing: Space.s8,
                children: [
                  for (final choice in result.choices)
                    TalvioChip(
                      label: choice,
                      onPressed: enabled ? () => onAnswer(choice) : null,
                    ),
                ],
              ),
            ],
            const SizedBox(height: Space.s8),
            _AnswerField(
              controller: answer,
              enabled: enabled,
              onAnswer: onAnswer,
            ),
          ],
          if (error != null && onRetry != null) ...[
            const SizedBox(height: Space.s12),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: Btn(
                CommandStrings.tryAgain,
                kind: BtnKind.tonal,
                compact: true,
                icon: AppIcons.retry,
                onPressed: () {
                  Haptic.tap();
                  onRetry!();
                },
              ),
            ),
          ],
        ],
      ),
    );
    if (waiting) return card;
    // The close button's glyph sits on the card's 16 padding.
    return Stack(
      children: [
        card,
        PositionedDirectional(
          top: 4,
          end: Space.gutter + 4,
          child: IconBtn(
            icon: AppIcons.close,
            tooltip: CommandStrings.dismiss,
            color: t.muted,
            onPressed: onDismiss,
          ),
        ),
      ],
    );
  }
}

/// The command as it was said, one line; a tap shows all of it.
class _Said extends StatefulWidget {
  const _Said(this.said);

  final String said;

  @override
  State<_Said> createState() => _SaidState();
}

class _SaidState extends State<_Said> {
  var _open = false;

  @override
  Widget build(BuildContext context) => GestureDetector(
    onTap: () => setState(() => _open = !_open),
    child: Text(
      CommandStrings.said(widget.said),
      maxLines: _open ? null : 1,
      overflow: _open ? null : TextOverflow.ellipsis,
      style: TalvioType.footnote.copyWith(color: context.talvio.muted),
    ),
  );
}

/// The answer to a command's question, typed.
class _AnswerField extends StatelessWidget {
  const _AnswerField({
    required this.controller,
    required this.enabled,
    required this.onAnswer,
  });

  final TextEditingController controller;
  final bool enabled;
  final ValueChanged<String> onAnswer;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Container(
      constraints: const BoxConstraints(minHeight: 44),
      decoration: BoxDecoration(
        color: t.fill,
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: controller,
              enabled: enabled,
              textCapitalization: TextCapitalization.sentences,
              textInputAction: TextInputAction.send,
              onSubmitted: onAnswer,
              onTapOutside: (_) =>
                  FocusManager.instance.primaryFocus?.unfocus(),
              style: TalvioType.body.copyWith(color: t.ink),
              decoration: InputDecoration(
                hintText: CommandStrings.answerHint,
                hintStyle: TalvioType.body.copyWith(color: t.muted),
                filled: false,
                isDense: true,
                border: InputBorder.none,
                enabledBorder: InputBorder.none,
                focusedBorder: InputBorder.none,
                disabledBorder: InputBorder.none,
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: 12,
                  vertical: 10,
                ),
              ),
            ),
          ),
          ValueListenableBuilder(
            valueListenable: controller,
            builder: (context, value, _) => IconBtn(
              icon: AppIcons.send,
              tooltip: CommandStrings.answer,
              onPressed: enabled && value.text.trim().isNotEmpty
                  ? () => onAnswer(value.text)
                  : null,
            ),
          ),
        ],
      ),
    );
  }
}

class _RecentRow extends StatelessWidget {
  const _RecentRow(this.item, {required this.onRunAgain});

  final RecentCommand item;
  final VoidCallback? onRunAgain;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final (IconData icon, Color color) = switch (item.status) {
      CommandStatus.done => (AppIcons.done, t.success),
      CommandStatus.failed => (AppIcons.failed, t.danger),
      CommandStatus.cancelled => (AppIcons.cancelledFill, t.muted),
      CommandStatus.awaitingConfirmation => (AppIcons.waitingFill, t.accent),
      CommandStatus.needsAnswer => (AppIcons.questionFill, t.accent),
      CommandStatus.needsComputerUse => (AppIcons.needsScreenFill, t.warning),
      _ => (AppIcons.done, t.muted),
    };
    return ListRow(
      leading: RowGlyph(icon, color: color),
      title: item.said,
      titleMaxLines: 1,
      subtitle: item.feedback.isEmpty ? null : item.feedback,
      trailing: IconBtn(
        icon: AppIcons.retry,
        tooltip: CommandStrings.runAgain(item.said),
        color: t.muted,
        onPressed: onRunAgain,
      ),
    );
  }
}
