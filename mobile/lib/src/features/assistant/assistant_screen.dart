import 'dart:async';

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../../shell/feature_tab.dart';
import '../dictate/computer_widgets.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import '../dictate/strings.dart';
import '../screen/live_controller.dart';
import 'assistant_controller.dart';
import 'assistant_scope.dart';
import 'assistant_widgets.dart';
import 'strings.dart';

/// Asking the computer's assistant, which can also do things there.
final assistantTab = FeatureTab(
  id: 'assistant',
  label: AssistantStrings.tab,
  icon: AppIcons.ask,
  selectedIcon: AppIcons.askSelected,
  requires: AssistantKind.capability,
  builder: (_) => const AssistantScreen(),
  actions: (context) => [const _NewChatButton()],
  composer: true,
);

/// An icon for each of [AssistantStrings.exampleQuestions].
const _exampleIcons = [AppIcons.music, AppIcons.weather, AppIcons.mail];

class _NewChatButton extends StatelessWidget {
  const _NewChatButton();

  @override
  Widget build(BuildContext context) {
    final c = AssistantScope.of(context);
    return ListenableBuilder(
      listenable: c,
      builder: (context, _) => IconBtn(
        icon: AppIcons.newChat,
        size: IconSize.large,
        tooltip: AssistantStrings.newChat,
        onPressed: c.turns.isEmpty || c.phase == AssistantPhase.sending
            ? null
            : () {
                Haptic.tick();
                unawaited(c.newChat());
              },
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
  String? _lastError;

  /// Far enough from the latest answer to offer a way back to it.
  var _away = false;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_follow);
    _scroll.addListener(_onScroll);
  }

  @override
  void dispose() {
    _controller.removeListener(_follow);
    _scroll.dispose();
    super.dispose();
  }

  void _onScroll() {
    final away =
        _scroll.hasClients &&
        _scroll.position.maxScrollExtent - _scroll.offset > 200;
    if (away != _away) setState(() => _away = away);
  }

  void _toLatest() {
    Haptic.tick();
    if (!_scroll.hasClients) return;
    unawaited(
      _scroll.animateTo(
        _scroll.position.maxScrollExtent,
        duration: Motion.emphasized,
        curve: Motion.emphasizedCurve,
      ),
    );
  }

  /// Keeps the newest answer in view as it streams in.
  void _follow() {
    final turns = _controller.turns;
    final error = turns.lastOrNull?.error;
    if (error != null && error != _lastError && error != 'cancelled') {
      Haptic.error();
    }
    _lastError = error;
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
            duration: Motion.standard,
            curve: Motion.enter,
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
    final live = LiveScope.maybeOf(context);
    return ListenableBuilder(
      listenable: Listenable.merge([
        _controller,
        relay.devices,
        relay.status,
        ?live,
      ]),
      builder: (context, _) =>
          ComputerGate(controller: _dictate, builder: _buildBody),
    );
  }

  Widget _buildBody(BuildContext context, Device computer, bool connected) {
    final c = _controller;
    final name = computer.name;
    final ready = connected && computer.online && hasAssistant(computer);
    final outdated =
        computer.online &&
        connected &&
        computer.capabilities.isNotEmpty &&
        !hasAssistant(computer);
    final turns = c.turns;
    final watching = LiveScope.maybeOf(context)?.watch != null;

    final Widget body;
    if (turns.isEmpty && !computer.online) {
      body = EmptyState(
        icon: DuoIcons.offline,
        title: DictateStrings.offlineTitle(name),
        body: AssistantStrings.offlineBody(name),
      );
    } else if (turns.isEmpty && outdated) {
      body = EmptyState(
        icon: DuoIcons.update,
        title: AssistantStrings.updateTitle(name),
        body: AssistantStrings.updateNote(name),
      );
    } else if (turns.isEmpty) {
      body = _Empty(
        name: name,
        heading: !watching,
        onAsk: ready && c.idle
            ? (text) {
                Haptic.tap();
                unawaited(c.ask(text));
              }
            : null,
      );
    } else {
      final note = !computer.online
          ? AssistantStrings.offlineNote(name)
          : outdated
          ? AssistantStrings.updateNote(name)
          : null;
      body = Stack(
        children: [
          ListView(
            controller: _scroll,
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 24),
            children: [
              if (note != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: Space.s24),
                  child: InlineBanner(
                    margin: EdgeInsets.zero,
                    icon: computer.online ? AppIcons.update : AppIcons.offline,
                    message: note,
                  ),
                ),
              for (final (i, turn) in turns.indexed)
                Padding(
                  padding: EdgeInsets.only(
                    bottom: i == turns.length - 1 ? 0 : Space.s24,
                  ),
                  child: Appear(
                    key: ObjectKey(turn),
                    child: TurnView(turn: turn, controller: c),
                  ),
                ),
            ],
          ),
          PositionedDirectional(
            start: 0,
            end: 0,
            bottom: 12,
            child: Center(
              child: AnimatedSwitcher(
                duration: Motion.quick,
                child: _away
                    ? _LatestButton(onPressed: _toLatest)
                    : const SizedBox.shrink(),
              ),
            ),
          ),
        ],
      );
    }

    return Column(
      children: [
        Expanded(child: body),
        AssistantInput(
          controller: c,
          enabled: ready,
          offlineName: computer.online ? null : name,
        ),
      ],
    );
  }
}

/// A new conversation: what the assistant is, and three questions to tap,
/// right above the composer that sends them.
class _Empty extends StatelessWidget {
  const _Empty({
    required this.name,
    required this.heading,
    required this.onAsk,
  });

  final String name;

  /// Show the heading (not while the live view takes the room).
  final bool heading;
  final ValueChanged<String>? onAsk;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final suggestions = Group(
      children: [
        for (final (i, example) in AssistantStrings.exampleQuestions.indexed)
          ListRow(
            leading: RowGlyph(_exampleIcons[i % _exampleIcons.length]),
            title: example,
            subtitle: AssistantStrings.exampleDetails[i],
            enabled: onAsk != null,
            semanticLabel: example,
            semanticValue: AssistantStrings.exampleDetails[i],
            onTap: onAsk == null ? null : () => onAsk!(example),
          ),
      ],
    );
    final headingBlock = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 32),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            AssistantStrings.prompt(name),
            textAlign: TextAlign.center,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: TalvioType.title1.copyWith(color: t.ink),
          ),
          const SizedBox(height: Space.s8),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 300),
            child: Text(
              AssistantStrings.promptDetail,
              textAlign: TextAlign.center,
              style: TalvioType.rowBody.copyWith(color: t.muted),
            ),
          ),
        ],
      ),
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        // The heading goes first when room is short (keyboard, large text).
        final scale = MediaQuery.textScalerOf(context).scale(1);
        final roomy = heading && constraints.maxHeight >= 360 * scale;
        if (roomy) {
          return Column(
            children: [
              Expanded(
                child: LayoutBuilder(
                  builder: (context, room) => room.maxHeight >= 180 * scale
                      ? Center(child: headingBlock)
                      : const SizedBox.expand(),
                ),
              ),
              suggestions,
              const SizedBox(height: Space.s12),
            ],
          );
        }
        return SingleChildScrollView(
          reverse: true,
          padding: const EdgeInsets.only(top: Space.s16, bottom: Space.s12),
          child: suggestions,
        );
      },
    );
  }
}

/// Back to the newest answer, centred above the composer.
class _LatestButton extends StatelessWidget {
  const _LatestButton({required this.onPressed});

  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Semantics(
      button: true,
      label: AssistantStrings.scrollToLatest,
      onTap: onPressed,
      excludeSemantics: true,
      child: Tooltip(
        message: AssistantStrings.scrollToLatest,
        excludeFromSemantics: true,
        child: GestureDetector(
          onTap: onPressed,
          child: SizedBox.square(
            dimension: 44,
            child: Center(
              child: Container(
                width: 36,
                height: 36,
                decoration: BoxDecoration(
                  color: t.card,
                  shape: BoxShape.circle,
                  border: Border.all(color: t.border),
                ),
                child: Icon(AppIcons.caretDown, size: 20, color: t.ink),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
