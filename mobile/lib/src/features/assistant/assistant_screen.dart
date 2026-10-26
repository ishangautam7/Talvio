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
const _exampleIcons = [
  AppIcons.music,
  AppIcons.mail,
  AppIcons.weather,
  AppIcons.globe,
];

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

  /// The signed-in user's first name, for the greeting.
  String? _firstName() {
    final name = _services.session.session?.user.name?.trim();
    if (name == null || name.isEmpty) return null;
    return name.split(RegExp(r'\s+')).first;
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
        userName: _firstName(),
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

/// A new conversation: the orb, a greeting, and three questions to tap
/// right above the composer that sends them.
class _Empty extends StatelessWidget {
  const _Empty({
    required this.name,
    required this.userName,
    required this.heading,
    required this.onAsk,
  });

  final String name;
  final String? userName;

  /// Show the orb (not while the live view takes the room; the greeting
  /// stays when there is space for it).
  final bool heading;
  final ValueChanged<String>? onAsk;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final suggestions = _Suggestions(onAsk: onAsk);
    final greeting = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 32),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (heading) ...[
            const TalvioOrb(size: 100),
            const SizedBox(height: Space.s40),
          ],
          Semantics(
            header: true,
            child: Text(
              AssistantStrings.greeting(DateTime.now().hour, userName),
              textAlign: TextAlign.center,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: TalvioType.display.copyWith(color: t.ink),
            ),
          ),
          const SizedBox(height: Space.s8),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 300),
            child: Text(
              AssistantStrings.greetingDetail(name),
              textAlign: TextAlign.center,
              style: TalvioType.rowBody.copyWith(color: t.muted),
            ),
          ),
        ],
      ),
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        // The greeting goes first when room is short (keyboard, large text).
        final scale = MediaQuery.textScalerOf(context).scale(1);
        final roomy = constraints.maxHeight >= (heading ? 440 : 340) * scale;
        if (roomy) {
          return Stack(
            children: [
              // A faint light at the top of the page, like the desktop's
              // canvas: the orb's colour, far too soft to read as a shape.
              Positioned.fill(
                child: IgnorePointer(
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      gradient: RadialGradient(
                        // Faded out well before the top bar, so no seam.
                        center: const Alignment(0, -0.12),
                        radius: 0.7,
                        colors: [
                          t.coral.withValues(alpha: t.isDark ? 0.10 : 0.07),
                          t.coral.withValues(alpha: 0),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
              Column(
                children: [
                  Expanded(
                    child: LayoutBuilder(
                      builder: (context, room) =>
                          room.maxHeight >= (heading ? 260 : 120) * scale
                          ? Center(child: greeting)
                          : const SizedBox.expand(),
                    ),
                  ),
                  suggestions,
                ],
              ),
            ],
          );
        }
        return SingleChildScrollView(
          reverse: true,
          padding: const EdgeInsets.only(top: Space.s16),
          child: suggestions,
        );
      },
    );
  }
}

/// The example questions as a row of cards that scrolls sideways.
class _Suggestions extends StatelessWidget {
  const _Suggestions({required this.onAsk});

  final ValueChanged<String>? onAsk;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final scale = MediaQuery.textScalerOf(context).scale(1);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Padding(
          padding: const EdgeInsetsDirectional.fromSTEB(20, 0, 20, Space.s8),
          child: Text(
            AssistantStrings.examples,
            style: TalvioType.label.copyWith(color: t.muted),
          ),
        ),
        SizedBox(
          height: 128 * scale + 20,
          child: ListView.separated(
            scrollDirection: Axis.horizontal,
            clipBehavior: Clip.none,
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
            itemCount: AssistantStrings.exampleQuestions.length,
            separatorBuilder: (_, _) => const SizedBox(width: Space.s12),
            itemBuilder: (context, i) {
              final example = AssistantStrings.exampleQuestions[i];
              return _SuggestionCard(
                width: (168 * scale).clamp(168, 216),
                titleLines: scale > 1.1 ? 3 : 2,
                icon: _exampleIcons[i % _exampleIcons.length],
                title: example,
                detail: AssistantStrings.exampleDetails[i],
                onTap: onAsk == null ? null : () => onAsk!(example),
              );
            },
          ),
        ),
      ],
    );
  }
}

class _SuggestionCard extends StatelessWidget {
  const _SuggestionCard({
    required this.width,
    required this.titleLines,
    required this.icon,
    required this.title,
    required this.detail,
    required this.onTap,
  });

  final double width;
  final int titleLines;
  final IconData icon;
  final String title;
  final String detail;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = onTap != null;
    return Semantics(
      button: true,
      enabled: enabled,
      label: title,
      value: detail,
      onTap: onTap,
      excludeSemantics: true,
      child: AnimatedOpacity(
        duration: Motion.quick,
        opacity: enabled ? 1 : 0.5,
        child: Pressable(
          enabled: enabled,
          child: Container(
            width: width,
            decoration: BoxDecoration(
              color: t.card,
              borderRadius: BorderRadius.circular(Radii.xl),
              boxShadow: Depth.card(t),
              border: Depth.edge(t),
            ),
            child: Material(
              type: MaterialType.transparency,
              borderRadius: BorderRadius.circular(Radii.xl),
              clipBehavior: Clip.antiAlias,
              child: InkWell(
                onTap: onTap,
                child: Padding(
                  padding: const EdgeInsets.all(14),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Container(
                            width: 32,
                            height: 32,
                            decoration: BoxDecoration(
                              color: t.fill,
                              shape: BoxShape.circle,
                            ),
                            child: Icon(icon, size: 16, color: t.ink),
                          ),
                          const Spacer(),
                          Icon(AppIcons.arrowUpRight, size: 16, color: t.faint),
                        ],
                      ),
                      const Spacer(),
                      Text(
                        title,
                        maxLines: titleLines,
                        overflow: TextOverflow.ellipsis,
                        style: TalvioType.rowTitle.copyWith(color: t.ink),
                      ),
                      const SizedBox(height: Space.s2),
                      Text(
                        detail,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TalvioType.footnote.copyWith(color: t.muted),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
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
