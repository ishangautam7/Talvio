import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/widgets.dart';
import '../../shell/feature_tab.dart';
import '../../shell/shell_scope.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import '../dictate/mic_widgets.dart';
import '../dictate/strings.dart';
import 'command_controller.dart';
import 'command_scope.dart';
import 'recent_commands.dart';
import 'strings.dart';

/// Telling the computer what to do: "open YouTube", "play a song".
final doTab = FeatureTab(
  id: 'do',
  label: CommandStrings.tab,
  icon: Icons.auto_awesome_outlined,
  selectedIcon: Icons.auto_awesome,
  requires: CommandKind.capability,
  builder: (_) => const CommandScreen(),
);

class CommandScreen extends StatefulWidget {
  const CommandScreen({super.key});

  @override
  State<CommandScreen> createState() => _CommandScreenState();
}

class _CommandScreenState extends State<CommandScreen> {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _dictate = DictateScope.of(context);
  late final CommandController _controller = CommandScope.of(context);
  final _text = TextEditingController();
  final _textFocus = FocusNode();
  final _answer = TextEditingController();

  @override
  void dispose() {
    _text.dispose();
    _textFocus.dispose();
    _answer.dispose();
    super.dispose();
  }

  Future<void> _send(String text) async {
    if (text.trim().isEmpty) return;
    _textFocus.unfocus();
    unawaited(HapticFeedback.lightImpact());
    if (await _controller.sendText(text) && mounted) _text.clear();
  }

  Future<void> _sendAnswer(String text) async {
    if (text.trim().isEmpty) return;
    FocusManager.instance.primaryFocus?.unfocus();
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
        _textFocus,
      ]),
      builder: (context, _) {
        final computer = _controller.computer;
        return Scaffold(
          appBar: AppBar(
            leading: shellLeading(context),
            title: computer == null
                ? const Text(CommandStrings.title)
                : ComputerTitle(
                    computer,
                    onTap: canChooseComputer(_dictate) && _controller.idle
                        ? () => chooseComputer(context, _dictate)
                        : null,
                  ),
          ),
          body: ComputerGate(controller: _dictate, builder: _buildBody),
        );
      },
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool connected) {
    final c = _controller;
    final idle = c.idle;
    final ready =
        connected &&
        computer.online &&
        computer.capabilities.contains(CommandKind.capability);
    final typing = _textFocus.hasFocus;
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

    final progress = c.cancelling
        ? CommandStrings.cancelling
        : switch (c.stage) {
            'approval' => DictateStrings.approval(name),
            'transcribing' => CommandStrings.transcribing(name),
            'working' => CommandStrings.working(name),
            _ => CommandStrings.sending(name),
          };
    final recents = c.recents.items;

    return LayoutBuilder(
      builder: (context, constraints) => Column(
        children: [
          Expanded(
            child: ListView(
              padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
              children: [
                if (note != null && c.phase != CommandPhase.sending)
                  _Note(note),
                if (c.phase == CommandPhase.sending)
                  _ProgressCard(
                    text: progress,
                    detail: c.stage == 'approval'
                        ? DictateStrings.approvalDetail
                        : null,
                    onCancel: c.cancelling ? null : c.cancelRequest,
                  )
                else if (outcome != null)
                  _ResultCard(
                    outcome: outcome,
                    computerName: outcome.computer.isEmpty
                        ? name
                        : outcome.computer,
                    enabled: ready && idle,
                    answer: _answer,
                    onConfirm: (accept) => c.confirm(accept: accept),
                    onAnswer: _sendAnswer,
                    onRetry: c.canRetry && ready ? c.retry : null,
                    onDismiss: c.dismissOutcome,
                  )
                else if (note == null)
                  _Prompt(name),
                if (c.phase == CommandPhase.idle) ...[
                  const SectionTitle(CommandStrings.examples),
                  Wrap(
                    spacing: 8,
                    runSpacing: 4,
                    children: [
                      for (final example in CommandStrings.exampleCommands)
                        ActionChip(
                          avatar: const Icon(Icons.auto_awesome, size: 18),
                          label: Text(example),
                          onPressed: ready && idle
                              ? () => unawaited(_send(example))
                              : null,
                        ),
                    ],
                  ),
                ],
                if (recents.isNotEmpty) ...[
                  const SectionTitle(CommandStrings.recent),
                  for (final item in recents)
                    _RecentTile(
                      item,
                      onRunAgain: ready && idle
                          ? () => unawaited(_send(item.said))
                          : null,
                    ),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                    child: Text(
                      CommandStrings.recentNote,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: Theme.of(context).colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
          // As on Send: short screens scroll the controls, and while typing
          // the controls are the text field.
          ConstrainedBox(
            constraints: BoxConstraints(
              maxHeight: typing
                  ? constraints.maxHeight
                  : math.max(
                      constraints.maxHeight - 96,
                      constraints.maxHeight / 2,
                    ),
            ),
            child: SingleChildScrollView(
              child: SafeArea(
                top: false,
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
                  child: Column(
                    children: [
                      if (!typing) ...[
                        MicButton(
                          recording: c.phase == CommandPhase.recording,
                          busy: c.phase == CommandPhase.sending,
                          level: c.level,
                          startLabel: CommandStrings.startRecording,
                          stopLabel: CommandStrings.stopAndSend,
                          onPressed: switch (c.phase) {
                            CommandPhase.idle when ready && idle =>
                              c.startRecording,
                            CommandPhase.recording => c.stopAndSend,
                            _ => null,
                          },
                        ),
                        ConstrainedBox(
                          constraints: const BoxConstraints(minHeight: 48),
                          child: c.phase == CommandPhase.recording
                              ? RecordingCaption(
                                  elapsed: c.elapsed,
                                  paused: c.paused,
                                  onDiscard: c.discardRecording,
                                )
                              : Center(
                                  child: Text(
                                    c.phase == CommandPhase.idle
                                        ? CommandStrings.tapToSpeak
                                        : '',
                                    style: mutedText(context),
                                  ),
                                ),
                        ),
                      ],
                      _Composer(
                        controller: _text,
                        focusNode: _textFocus,
                        enabled: ready && idle,
                        onSend: () => _send(_text.text),
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

class _Note extends StatelessWidget {
  const _Note(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Card.filled(
    child: ListTile(leading: const Icon(Icons.info_outline), title: Text(text)),
  );
}

class _Prompt extends StatelessWidget {
  const _Prompt(this.name);

  final String name;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 8, 8, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            CommandStrings.prompt(name),
            style: theme.textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(CommandStrings.promptDetail, style: mutedText(context)),
        ],
      ),
    );
  }
}

class _ProgressCard extends StatelessWidget {
  const _ProgressCard({
    required this.text,
    required this.detail,
    required this.onCancel,
  });

  final String text;
  final String? detail;
  final VoidCallback? onCancel;

  @override
  Widget build(BuildContext context) {
    return Card.outlined(
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const LinearProgressIndicator(),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
            child: Row(
              children: [
                Expanded(
                  child: Semantics(
                    liveRegion: true,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(text),
                        if (detail case final detail?)
                          Text(detail, style: mutedText(context)),
                      ],
                    ),
                  ),
                ),
                TextButton(
                  onPressed: onCancel,
                  child: const Text(CommandStrings.cancel),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// What came of the last command: the computer's result, or why the
/// request failed.
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
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final result = outcome.result;
    final error = outcome.error;
    final failed = error != null || result?.status == CommandStatus.failed;
    final onCard = failed ? scheme.onErrorContainer : scheme.onSurface;

    final (IconData icon, String title) = switch (result?.status) {
      _ when error != null => (Icons.error_outline, CommandStrings.failed),
      CommandStatus.awaitingConfirmation => (
        Icons.help_outline,
        CommandStrings.awaitingConfirmation,
      ),
      CommandStatus.needsAnswer => (
        Icons.question_answer_outlined,
        CommandStrings.needsAnswer,
      ),
      CommandStatus.cancelled => (Icons.block, CommandStrings.cancelled),
      CommandStatus.failed => (Icons.error_outline, CommandStrings.failed),
      CommandStatus.needsComputerUse => (
        Icons.mouse_outlined,
        CommandStrings.needsComputerUse,
      ),
      _ => (Icons.check_circle_outline, CommandStrings.done),
    };

    final body = <Widget>[
      if (outcome.said case final said?)
        Padding(
          padding: const EdgeInsets.only(bottom: 6),
          child: Text(
            CommandStrings.said(said),
            style: theme.textTheme.bodyMedium?.copyWith(
              fontStyle: FontStyle.italic,
              color: failed ? onCard : scheme.onSurfaceVariant,
            ),
          ),
        ),
      if (error != null)
        Text(
          CommandStrings.error(error, computerName),
          style: theme.textTheme.bodyLarge?.copyWith(color: onCard),
        )
      else if (result != null) ...[
        if (result.feedback.isNotEmpty)
          Text(
            result.feedback,
            style: theme.textTheme.bodyLarge?.copyWith(color: onCard),
          ),
        if (result.status == CommandStatus.needsComputerUse)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(
              CommandStrings.needsComputerUseBody,
              style: mutedText(context),
            ),
          ),
        if (result.confirmable) ...[
          if (result.pending case final pending?)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: scheme.secondaryContainer,
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Text(
                    pending,
                    style: theme.textTheme.titleSmall?.copyWith(
                      color: scheme.onSecondaryContainer,
                    ),
                  ),
                ),
              ),
            ),
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: OverflowBar(
              alignment: MainAxisAlignment.end,
              spacing: 8,
              children: [
                TextButton(
                  onPressed: enabled ? () => onConfirm(false) : null,
                  child: const Text(CommandStrings.cancel),
                ),
                FilledButton(
                  onPressed: enabled ? () => onConfirm(true) : null,
                  child: const Text(CommandStrings.doIt),
                ),
              ],
            ),
          ),
        ],
        if (result.answerable) ...[
          if (result.question case final question?)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(question, style: theme.textTheme.titleMedium),
            ),
          if (result.choices.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  for (final choice in result.choices)
                    FilledButton.tonal(
                      onPressed: enabled ? () => onAnswer(choice) : null,
                      child: Text(choice),
                    ),
                ],
              ),
            ),
          Padding(
            padding: const EdgeInsets.only(top: 8),
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
                const SizedBox(width: 8),
                ValueListenableBuilder(
                  valueListenable: answer,
                  builder: (context, value, _) => IconButton.filled(
                    tooltip: CommandStrings.answer,
                    onPressed: enabled && value.text.trim().isNotEmpty
                        ? () => onAnswer(value.text)
                        : null,
                    icon: const Icon(Icons.send),
                  ),
                ),
              ],
            ),
          ),
        ],
      ],
      if (error != null && onRetry != null)
        OverflowBar(
          alignment: MainAxisAlignment.end,
          children: [
            TextButton(
              onPressed: onRetry,
              child: const Text(CommandStrings.tryAgain),
            ),
          ],
        ),
    ];

    final content = Padding(
      padding: const EdgeInsets.fromLTRB(16, 8, 8, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(
                icon,
                size: 20,
                color: failed ? scheme.onErrorContainer : scheme.primary,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Semantics(
                  liveRegion: true,
                  child: Text(
                    title,
                    style: theme.textTheme.titleSmall?.copyWith(color: onCard),
                  ),
                ),
              ),
              IconButton(
                tooltip: CommandStrings.dismiss,
                icon: Icon(Icons.close, color: onCard),
                onPressed: onDismiss,
              ),
            ],
          ),
          Padding(
            padding: const EdgeInsets.only(right: 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: body,
            ),
          ),
        ],
      ),
    );
    return failed
        ? Card.filled(color: scheme.errorContainer, child: content)
        : Card.outlined(child: content);
  }
}

class _RecentTile extends StatelessWidget {
  const _RecentTile(this.item, {required this.onRunAgain});

  final RecentCommand item;
  final VoidCallback? onRunAgain;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final failed = item.status == CommandStatus.failed;
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 8),
      leading: Icon(switch (item.status) {
        CommandStatus.done => Icons.check_circle_outline,
        CommandStatus.failed => Icons.error_outline,
        CommandStatus.cancelled => Icons.block,
        _ => Icons.more_horiz,
      }, color: failed ? scheme.error : scheme.onSurfaceVariant),
      title: Text(item.said, maxLines: 2, overflow: TextOverflow.ellipsis),
      subtitle: item.feedback.isEmpty
          ? null
          : Text(item.feedback, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: IconButton(
        tooltip: CommandStrings.runAgain(item.said),
        onPressed: onRunAgain,
        icon: const Icon(Icons.replay),
      ),
    );
  }
}

/// The text field and Go. Commands are at most [maxCommandChars].
class _Composer extends StatelessWidget {
  const _Composer({
    required this.controller,
    required this.focusNode,
    required this.enabled,
    required this.onSend,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final bool enabled;
  final VoidCallback onSend;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        Expanded(
          child: TextField(
            controller: controller,
            focusNode: focusNode,
            enabled: enabled,
            minLines: 1,
            maxLines: 3,
            inputFormatters: [_commandLimit],
            textCapitalization: TextCapitalization.sentences,
            textInputAction: TextInputAction.go,
            onSubmitted: (_) => onSend(),
            onTapOutside: (_) => focusNode.unfocus(),
            decoration: const InputDecoration(
              hintText: CommandStrings.typeHint,
            ),
          ),
        ),
        const SizedBox(width: 8),
        ValueListenableBuilder(
          valueListenable: controller,
          builder: (context, value, _) => Semantics(
            label: CommandStrings.sendCommand,
            button: true,
            excludeSemantics: true,
            enabled: enabled && value.text.trim().isNotEmpty,
            onTap: enabled && value.text.trim().isNotEmpty ? onSend : null,
            child: FilledButton(
              onPressed: enabled && value.text.trim().isNotEmpty
                  ? onSend
                  : null,
              style: FilledButton.styleFrom(
                minimumSize: const Size(64, 56),
                padding: const EdgeInsets.symmetric(horizontal: 16),
              ),
              child: const Text(CommandStrings.go),
            ),
          ),
        ),
      ],
    );
  }
}

final _commandLimit = TextInputFormatter.withFunction((old, next) {
  if (next.text.runes.length <= maxCommandChars) return next;
  final text = String.fromCharCodes(next.text.runes.take(maxCommandChars));
  return TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
  );
});
