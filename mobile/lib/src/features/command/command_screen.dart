import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../design/voice.dart';
import '../../shell/feature_tab.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_parts.dart' show ProgressCard;
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
  icon: Icons.bolt_outlined,
  selectedIcon: Icons.bolt_rounded,
  requires: CommandKind.capability,
  builder: (_) => const CommandScreen(),
);

/// An icon for each of [CommandStrings.exampleCommands].
const _exampleIcons = [
  Icons.smart_display_outlined,
  Icons.music_note_rounded,
  Icons.pause_circle_outline_rounded,
  Icons.volume_down_rounded,
  Icons.mail_outline_rounded,
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

  @override
  void dispose() {
    _answer.dispose();
    super.dispose();
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
      ]),
      builder: (context, _) => ComputerGate(
        controller: DictateScope.of(context),
        builder: _buildBody,
      ),
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool connected) {
    final c = _controller;
    final idle = c.idle;
    final ready =
        connected &&
        computer.online &&
        computer.capabilities.contains(CommandKind.capability);
    final outcome = c.outcome;
    final name = computer.name;

    // A computer that advertises nothing yet has not said `hello`.
    final note = !computer.online
        ? CommandStrings.offlineNote(name)
        : connected &&
              computer.capabilities.isNotEmpty &&
              !computer.capabilities.contains(CommandKind.capability)
        ? CommandStrings.updateNote(name)
        : null;
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

    return Column(
      children: [
        Expanded(
          child: ListView(
            padding: const EdgeInsets.fromLTRB(
              Space.lg,
              Space.xs,
              Space.lg,
              Space.lg,
            ),
            children: [
              if (note != null && c.phase != CommandPhase.sending)
                Padding(
                  padding: const EdgeInsets.only(bottom: Space.md),
                  child: Notice(
                    tone: computer.online ? Tone.warning : Tone.neutral,
                    icon: computer.online
                        ? Icons.system_update_alt_rounded
                        : Icons.power_settings_new_rounded,
                    message: note,
                  ),
                ),
              if (c.phase == CommandPhase.sending)
                _Sending(controller: c, name: name)
              else if (outcome != null)
                _ResultCard(
                  outcome: outcome,
                  computerName: outcome.computer.isEmpty
                      ? name
                      : outcome.computer,
                  enabled: ready && idle,
                  answer: _answer,
                  onConfirm: (accept) {
                    accept ? Haptic.firm() : Haptic.tick();
                    unawaited(c.confirm(accept: accept));
                  },
                  onAnswer: _sendAnswer,
                  onRetry: c.canRetry && ready ? c.retry : null,
                  onDismiss: c.dismissOutcome,
                )
              else if (note == null && c.phase == CommandPhase.idle)
                _Prompt(name),
              if (c.phase == CommandPhase.idle && outcome == null) ...[
                const SectionHeader(CommandStrings.examples),
                _Examples(
                  onTap: ready && idle
                      ? (text) => unawaited(_send(text))
                      : null,
                ),
              ],
              if (recents.isNotEmpty) ...[
                SectionHeader(
                  CommandStrings.recent,
                  trailing: Text(
                    CommandStrings.recentShort,
                    style: context.text.bodySmall,
                  ),
                ),
                Surface(
                  padding: EdgeInsets.zero,
                  child: Column(
                    children: [
                      for (final (i, item) in recents.indexed) ...[
                        if (i > 0)
                          Divider(indent: 60, color: context.talvio.hairline),
                        _RecentTile(
                          item,
                          onRunAgain: ready && idle
                              ? () => unawaited(_send(item.said))
                              : null,
                        ),
                      ],
                    ],
                  ),
                ),
              ],
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(Space.lg, 0, Space.lg, Space.md),
          child: Composer(
            hint: CommandStrings.typeHint,
            sendLabel: CommandStrings.sendCommand,
            voice: voice,
            enabled: ready && idle,
            busy: c.phase == CommandPhase.sending,
            maxLines: 3,
            // The field clears at once: the command shows in its card, and
            // Try again sends it again.
            onSubmit: (text) {
              unawaited(_send(text));
              return Future.value(true);
            },
          ),
        ),
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
      return ApprovalCard(
        title: DictateStrings.approval(name),
        body: DictateStrings.approvalDetail,
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
    return ProgressCard(
      text: text,
      detail: c.stage == 'working' && !c.cancelling
          ? CommandStrings.workingDetail
          : null,
      onCancel: c.cancelling ? null : c.cancelRequest,
      cancelLabel: CommandStrings.cancel,
    );
  }
}

class _Prompt extends StatelessWidget {
  const _Prompt(this.name);

  final String name;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(Space.xs, Space.xl, Space.xs, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const IconTile(
            Icons.bolt_rounded,
            size: 52,
            gradient: TalvioColors.gradient,
          ),
          const SizedBox(height: Space.lg),
          Text(CommandStrings.prompt(name), style: context.text.headlineMedium),
          const SizedBox(height: Space.sm),
          Text(CommandStrings.promptDetail, style: mutedText(context)),
        ],
      ),
    );
  }
}

/// Commands to tap and send as they are.
class _Examples extends StatelessWidget {
  const _Examples({required this.onTap});

  final ValueChanged<String>? onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Wrap(
      spacing: Space.sm,
      runSpacing: 0,
      children: [
        for (final (i, example) in CommandStrings.exampleCommands.indexed)
          ActionChip(
            avatar: Icon(
              _exampleIcons[i % _exampleIcons.length],
              size: 18,
              color: onTap == null ? t.faint : t.accent,
            ),
            label: Text(example),
            onPressed: onTap == null ? null : () => onTap!(example),
          ),
      ],
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

    final (IconData icon, Tone tone, String title) = switch (result?.status) {
      _ when error == 'cancelled' => (
        Icons.block_rounded,
        Tone.neutral,
        CommandStrings.cancelled,
      ),
      _ when error != null => (
        Icons.error_outline_rounded,
        Tone.danger,
        CommandStrings.failed,
      ),
      CommandStatus.awaitingConfirmation => (
        Icons.front_hand_outlined,
        Tone.accent,
        CommandStrings.awaitingConfirmation,
      ),
      CommandStatus.needsAnswer => (
        Icons.help_outline_rounded,
        Tone.accent,
        CommandStrings.needsAnswer,
      ),
      CommandStatus.cancelled => (
        Icons.block_rounded,
        Tone.neutral,
        CommandStrings.cancelled,
      ),
      CommandStatus.failed => (
        Icons.error_outline_rounded,
        Tone.danger,
        CommandStrings.failed,
      ),
      CommandStatus.needsComputerUse => (
        Icons.mouse_outlined,
        Tone.warning,
        CommandStrings.needsComputerUse,
      ),
      _ => (Icons.check_rounded, Tone.success, CommandStrings.done),
    };

    final body = <Widget>[
      if (outcome.said case final said?)
        Padding(
          padding: const EdgeInsets.only(bottom: Space.sm),
          child: Text(
            CommandStrings.said(said),
            style: context.text.bodyMedium?.copyWith(color: t.muted),
          ),
        ),
      if (error != null)
        Text(
          CommandStrings.error(error, computerName),
          style: context.text.bodyLarge,
        )
      else if (result != null) ...[
        if (result.feedback.isNotEmpty)
          Text(result.feedback, style: context.text.bodyLarge)
        else if (result.status == CommandStatus.needsComputerUse)
          Text(
            CommandStrings.needsComputerUseBody,
            style: context.text.bodyLarge,
          ),
        if (result.confirmable) ...[
          if (result.pending case final pending?)
            Padding(
              padding: const EdgeInsets.only(top: Space.md),
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: t.accentSoft,
                  borderRadius: BorderRadius.circular(Radii.md),
                ),
                child: Padding(
                  padding: const EdgeInsets.all(Space.md),
                  child: Row(
                    children: [
                      Icon(
                        Icons.arrow_forward_rounded,
                        size: 18,
                        color: t.accent,
                      ),
                      const SizedBox(width: Space.sm),
                      Expanded(
                        child: Text(pending, style: context.text.titleSmall),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          Padding(
            padding: const EdgeInsets.only(top: Space.md),
            child: Row(
              children: [
                Expanded(
                  child: OutlinedButton(
                    onPressed: enabled ? () => onConfirm(false) : null,
                    child: const Text(CommandStrings.cancel),
                  ),
                ),
                const SizedBox(width: Space.sm),
                Expanded(
                  child: FilledButton(
                    onPressed: enabled ? () => onConfirm(true) : null,
                    child: const Text(CommandStrings.doIt),
                  ),
                ),
              ],
            ),
          ),
        ],
        if (result.answerable) ...[
          if (result.question case final question?)
            Padding(
              padding: const EdgeInsets.only(top: Space.md),
              child: Text(question, style: context.text.titleMedium),
            ),
          if (result.choices.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: Space.sm),
              child: Wrap(
                spacing: Space.sm,
                runSpacing: Space.sm,
                children: [
                  for (final choice in result.choices)
                    FilledButton.tonal(
                      onPressed: enabled ? () => onAnswer(choice) : null,
                      style: FilledButton.styleFrom(
                        backgroundColor: t.sunken,
                        foregroundColor: t.ink,
                      ),
                      child: Text(choice),
                    ),
                ],
              ),
            ),
          Padding(
            padding: const EdgeInsets.only(top: Space.sm),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: answer,
                    enabled: enabled,
                    textCapitalization: TextCapitalization.sentences,
                    textInputAction: TextInputAction.send,
                    onSubmitted: onAnswer,
                    onTapOutside: (_) =>
                        FocusManager.instance.primaryFocus?.unfocus(),
                    decoration: const InputDecoration(
                      hintText: CommandStrings.answerHint,
                      isDense: true,
                    ),
                  ),
                ),
                const SizedBox(width: Space.sm),
                ValueListenableBuilder(
                  valueListenable: answer,
                  builder: (context, value, _) => IconButton.filled(
                    tooltip: CommandStrings.answer,
                    onPressed: enabled && value.text.trim().isNotEmpty
                        ? () => onAnswer(value.text)
                        : null,
                    icon: const Icon(Icons.arrow_upward_rounded),
                  ),
                ),
              ],
            ),
          ),
        ],
      ],
      if (error != null && onRetry != null)
        Padding(
          padding: const EdgeInsets.only(top: Space.sm),
          child: Align(
            alignment: AlignmentDirectional.centerEnd,
            child: FilledButton(
              onPressed: () {
                Haptic.tap();
                onRetry!();
              },
              child: const Text(CommandStrings.tryAgain),
            ),
          ),
        ),
    ];

    final danger = tone == Tone.danger;
    return Surface(
      color: danger ? t.dangerSoft : null,
      borderColor: danger ? t.dangerSoft : null,
      padding: const EdgeInsets.fromLTRB(
        Space.lg,
        Space.md,
        Space.xs,
        Space.lg,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              IconTile(icon, tone: tone, size: 32),
              const SizedBox(width: Space.md),
              Expanded(
                child: Semantics(
                  liveRegion: true,
                  child: Text(title, style: context.text.titleMedium),
                ),
              ),
              IconButton(
                tooltip: CommandStrings.dismiss,
                icon: Icon(Icons.close_rounded, color: t.muted),
                onPressed: onDismiss,
              ),
            ],
          ),
          const SizedBox(height: Space.sm),
          Padding(
            padding: const EdgeInsets.only(right: Space.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: body,
            ),
          ),
        ],
      ),
    );
  }
}

class _RecentTile extends StatelessWidget {
  const _RecentTile(this.item, {required this.onRunAgain});

  final RecentCommand item;
  final VoidCallback? onRunAgain;

  @override
  Widget build(BuildContext context) {
    final (IconData icon, Tone tone) = switch (item.status) {
      CommandStatus.done => (Icons.check_rounded, Tone.success),
      CommandStatus.failed => (Icons.error_outline_rounded, Tone.danger),
      CommandStatus.cancelled => (Icons.block_rounded, Tone.neutral),
      CommandStatus.awaitingConfirmation => (
        Icons.front_hand_outlined,
        Tone.accent,
      ),
      CommandStatus.needsAnswer => (Icons.help_outline_rounded, Tone.accent),
      _ => (Icons.bolt_rounded, Tone.neutral),
    };
    return ListTile(
      contentPadding: const EdgeInsets.only(left: Space.lg, right: Space.xs),
      leading: IconTile(icon, tone: tone, size: 32),
      title: Text(
        item.said,
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
        style: context.text.titleSmall,
      ),
      subtitle: item.feedback.isEmpty
          ? null
          : Text(item.feedback, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: IconButton(
        tooltip: CommandStrings.runAgain(item.said),
        onPressed: onRunAgain,
        icon: const Icon(Icons.replay_rounded),
      ),
    );
  }
}
