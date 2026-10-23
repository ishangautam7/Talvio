import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/widgets.dart';
import '../../shell/feature_tab.dart';
import '../../shell/shell_scope.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import 'assistant_controller.dart';
import 'assistant_scope.dart';
import 'assistant_widgets.dart';
import 'strings.dart';

/// Asking the computer's assistant, which can also do things there.
final assistantTab = FeatureTab(
  id: 'assistant',
  label: AssistantStrings.tab,
  icon: Icons.chat_bubble_outline,
  selectedIcon: Icons.chat_bubble,
  requires: AssistantKind.capability,
  builder: (_) => const AssistantScreen(),
);

class AssistantScreen extends StatefulWidget {
  const AssistantScreen({super.key});

  @override
  State<AssistantScreen> createState() => _AssistantScreenState();
}

class _AssistantScreenState extends State<AssistantScreen> {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _dictate = DictateScope.of(context);
  late final AssistantController _controller = AssistantScope.of(context);
  final _scroll = ScrollController();
  var _shown = 0;
  var _lastAnswer = '';

  @override
  void initState() {
    super.initState();
    _controller.addListener(_follow);
  }

  @override
  void dispose() {
    _controller.removeListener(_follow);
    _scroll.dispose();
    super.dispose();
  }

  /// Keeps the newest answer in view as it streams in.
  void _follow() {
    final turns = _controller.turns;
    final answer = turns.lastOrNull?.answer ?? '';
    if (turns.length == _shown && answer == _lastAnswer) return;
    _shown = turns.length;
    _lastAnswer = answer;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.jumpTo(_scroll.position.maxScrollExtent);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final relay = _services.relay;
    return ListenableBuilder(
      listenable: Listenable.merge([_controller, relay.devices, relay.status]),
      builder: (context, _) {
        final computer = _controller.computer;
        final c = _controller;
        return Scaffold(
          appBar: AppBar(
            leading: shellLeading(context),
            title: computer == null
                ? const Text(AssistantStrings.title)
                : ComputerTitle(
                    computer,
                    onTap: canChooseComputer(_dictate) && c.idle
                        ? () => chooseComputer(context, _dictate)
                        : null,
                  ),
            actions: [
              IconButton(
                tooltip: AssistantStrings.newChat,
                onPressed: c.turns.isEmpty || c.phase == AssistantPhase.sending
                    ? null
                    : () => unawaited(c.newChat()),
                icon: const Icon(Icons.add_comment_outlined),
              ),
            ],
          ),
          body: ComputerGate(controller: _dictate, builder: _buildBody),
        );
      },
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool connected) {
    final c = _controller;
    final name = computer.name;
    final ready = connected && computer.online && hasAssistant(computer);
    final note = !computer.online
        ? AssistantStrings.offlineNote(name)
        : connected &&
              computer.capabilities.isNotEmpty &&
              !hasAssistant(computer)
        ? AssistantStrings.updateNote(name)
        : null;
    final turns = c.turns;

    return Column(
      children: [
        Expanded(
          child: ListView(
            controller: _scroll,
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
            children: [
              if (note != null)
                Card.filled(
                  child: ListTile(
                    leading: const Icon(Icons.info_outline),
                    title: Text(note),
                  ),
                ),
              if (turns.isEmpty) ...[
                if (note == null) _Prompt(name),
                const SectionTitle(AssistantStrings.examples),
                Wrap(
                  spacing: 8,
                  runSpacing: 4,
                  children: [
                    for (final example in AssistantStrings.exampleQuestions)
                      ActionChip(
                        avatar: const Icon(Icons.chat_bubble_outline, size: 18),
                        label: Text(example),
                        onPressed: ready && c.idle
                            ? () => unawaited(c.ask(example))
                            : null,
                      ),
                  ],
                ),
              ],
              for (final turn in turns)
                Padding(
                  padding: const EdgeInsets.only(bottom: 20),
                  child: TurnView(turn: turn, controller: c),
                ),
            ],
          ),
        ),
        Material(
          color: Theme.of(context).colorScheme.surfaceContainerLow,
          child: SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(8, 8, 8, 8),
              child: AssistantInput(controller: c, enabled: ready),
            ),
          ),
        ),
      ],
    );
  }
}

class _Prompt extends StatelessWidget {
  const _Prompt(this.name);

  final String name;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 8, 8, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            AssistantStrings.prompt(name),
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 4),
          Text(AssistantStrings.promptDetail, style: mutedText(context)),
        ],
      ),
    );
  }
}
