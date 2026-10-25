import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../design/brand.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../shell/feature_tab.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import '../dictate/strings.dart';
import 'assistant_controller.dart';
import 'assistant_scope.dart';
import 'assistant_widgets.dart';
import 'strings.dart';

/// Asking the computer's assistant, which can also do things there.
final assistantTab = FeatureTab(
  id: 'assistant',
  label: AssistantStrings.tab,
  icon: Icons.auto_awesome_outlined,
  selectedIcon: Icons.auto_awesome_rounded,
  requires: AssistantKind.capability,
  builder: (_) => const AssistantScreen(),
  actions: (context) => [const _NewChatButton()],
);

/// An icon for each of [AssistantStrings.exampleQuestions].
const _exampleIcons = [
  Icons.music_note_rounded,
  Icons.wb_sunny_outlined,
  Icons.mail_outline_rounded,
];

class _NewChatButton extends StatelessWidget {
  const _NewChatButton();

  @override
  Widget build(BuildContext context) {
    final c = AssistantScope.of(context);
    return ListenableBuilder(
      listenable: c,
      builder: (context, _) => IconButton(
        tooltip: AssistantStrings.newChat,
        onPressed: c.turns.isEmpty || c.phase == AssistantPhase.sending
            ? null
            : () {
                Haptic.tick();
                unawaited(c.newChat());
              },
        icon: const Icon(Icons.edit_square),
      ),
    );
  }
}

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
    final added = turns.length != _shown;
    _shown = turns.length;
    _lastAnswer = answer;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!_scroll.hasClients) return;
      final end = _scroll.position.maxScrollExtent;
      if (added) {
        unawaited(
          _scroll.animateTo(
            end,
            duration: Motion.medium,
            curve: Motion.emphasized,
          ),
        );
      } else {
        _scroll.jumpTo(end);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final relay = _services.relay;
    return ListenableBuilder(
      listenable: Listenable.merge([_controller, relay.devices, relay.status]),
      builder: (context, _) =>
          ComputerGate(controller: _dictate, builder: _buildBody),
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
        if (!computer.online && turns.isEmpty)
          Expanded(
            child: EmptyState(
              icon: Icons.power_settings_new_rounded,
              tone: Tone.neutral,
              title: DictateStrings.offlineTitle(name),
              body: AssistantStrings.offlineBody(name),
            ),
          )
        else
          Expanded(
            child: FadeBottom(
              child: ListView(
                controller: _scroll,
                padding: const EdgeInsets.fromLTRB(
                  Space.lg,
                  Space.xs,
                  Space.lg,
                  Space.lg,
                ),
                children: [
                  if (note != null)
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
                  if (turns.isEmpty) ...[
                    if (note == null) _Prompt(name),
                    const SectionHeader(AssistantStrings.examples),
                    for (final (i, example)
                        in AssistantStrings.exampleQuestions.indexed)
                      Padding(
                        padding: const EdgeInsets.only(bottom: Space.sm),
                        child: _Suggestion(
                          icon: _exampleIcons[i % _exampleIcons.length],
                          text: example,
                          detail: AssistantStrings.exampleDetails[i],
                          onTap: ready && c.idle
                              ? () {
                                  Haptic.tap();
                                  unawaited(c.ask(example));
                                }
                              : null,
                        ),
                      ),
                  ],
                  for (final turn in turns)
                    Padding(
                      padding: const EdgeInsets.only(bottom: Space.xxl),
                      child: Appear(
                        key: ObjectKey(turn),
                        child: TurnView(turn: turn, controller: c),
                      ),
                    ),
                ],
              ),
            ),
          ),
        Padding(
          padding: const EdgeInsets.fromLTRB(Space.lg, 0, Space.lg, Space.md),
          child: AssistantInput(controller: c, enabled: ready),
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
      padding: const EdgeInsets.fromLTRB(Space.xs, Space.xl, Space.xs, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const TalvioMark(size: 52),
          const SizedBox(height: Space.lg),
          Text(
            AssistantStrings.prompt(name),
            style: context.text.headlineMedium,
          ),
          const SizedBox(height: Space.sm),
          Text(AssistantStrings.promptDetail, style: mutedText(context)),
        ],
      ),
    );
  }
}

class _Suggestion extends StatelessWidget {
  const _Suggestion({
    required this.icon,
    required this.text,
    required this.detail,
    required this.onTap,
  });

  final IconData icon;
  final String text;
  final String detail;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Semantics(
      button: true,
      enabled: onTap != null,
      child: Surface(
        onTap: onTap,
        padding: const EdgeInsets.fromLTRB(
          Space.md,
          Space.md,
          Space.md,
          Space.md,
        ),
        child: Row(
          children: [
            IconTile(
              icon,
              tone: onTap == null ? Tone.neutral : Tone.accent,
              size: 36,
            ),
            const SizedBox(width: Space.md),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    text,
                    style: context.text.titleSmall?.copyWith(
                      color: onTap == null ? t.faint : t.ink,
                    ),
                  ),
                  Text(detail, style: context.text.bodySmall),
                ],
              ),
            ),
            Icon(Icons.arrow_outward_rounded, size: 18, color: t.faint),
          ],
        ),
      ),
    );
  }
}
