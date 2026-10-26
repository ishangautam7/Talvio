import 'dart:async';

import 'package:flutter/material.dart';

import '../core/app_services.dart';
import '../core/relay_client.dart';
import '../design/brand.dart';
import '../design/components.dart';
import '../design/tokens.dart';
import '../features/account/account_screen.dart';
import '../features/dictate/computer_widgets.dart';
import '../features/dictate/dictate_scope.dart';
import '../features/screen/live_controller.dart';
import '../features/screen/live_dock.dart';
import '../features/screen/screen_home.dart' show canWatch, watchMainScreen;
import '../features/screen/strings.dart';
import 'feature_tab.dart';
import 'shell_scope.dart';
import 'strings.dart';

/// The signed-in app: the computer at the top, the live view under it while
/// a watch runs, one page per available feature, and the navigation bar at
/// the bottom. A wide window (a tablet, a phone on its side) keeps the pages
/// in a rail instead.
class HomeShell extends StatefulWidget {
  const HomeShell({super.key, required this.tabs});

  /// The first available is home.
  final List<FeatureTab> tabs;

  /// From this width the navigation is a rail beside the page.
  static const railWidth = 640.0;

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  // By id, not index: pages come and go as computers connect.
  late String _selected = widget.tabs.first.id;

  /// The page below the top bar has scrolled from its top.
  var _scrolled = false;

  /// The page has more below the navigation bar.
  var _under = false;

  void _select(String id) {
    if (id == _selected) return;
    Haptic.tick();
    FocusManager.instance.primaryFocus?.unfocus();
    setState(() {
      _selected = id;
      _scrolled = false;
      _under = false;
    });
  }

  bool _onScroll(ScrollNotification n) {
    if (n.metrics.axis != Axis.vertical) return false;
    // A chat list scrolls from the bottom; it is "scrolled" whenever
    // anything sits above the top edge.
    final scrolled = n.metrics.extentBefore > 0.5;
    final under = n.metrics.extentAfter > 0.5;
    if (scrolled != _scrolled || under != _under) {
      setState(() {
        _scrolled = scrolled;
        _under = under;
      });
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    final relay = AppServices.of(context).relay;
    final dictate = DictateScope.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([relay.devices, dictate]),
      builder: (context, _) {
        final tabs = [
          for (final tab in widget.tabs)
            if (tab.availableWith(relay.devices.value ?? const [])) tab,
        ];
        final index = tabs.indexWhere((tab) => tab.id == _selected);
        final shown = index < 0 ? 0 : index;
        final current = tabs[shown];
        final home = tabs.first.id;
        final rail = MediaQuery.sizeOf(context).width >= HomeShell.railWidth;
        // With no computer there is one screen, not four identical ones.
        final navigable = tabs.length >= 2 && dictate.computer != null;

        // Every page keeps its place in the stack, available or not:
        // IndexedStack matches its children by position, so a page coming
        // or going in front of another would throw that one's state away
        // (a recording, typed text).
        final pages = IndexedStack(
          index: widget.tabs.indexWhere((tab) => tab.id == current.id),
          children: [
            for (final tab in widget.tabs)
              KeyedSubtree(
                key: ValueKey(tab.id),
                // Hidden pages keep their state but neither tick, paint nor
                // take taps (nor are found by tests).
                child: Offstage(
                  offstage: tab.id != current.id,
                  child: TickerMode(
                    enabled: tab.id == current.id,
                    child: Builder(builder: tab.builder),
                  ),
                ),
              ),
          ],
        );

        final body = SafeArea(
          bottom: false,
          child: Column(
            children: [
              _Header(
                actions: current.actions?.call(context) ?? const [],
                scrolled: _scrolled,
              ),
              const _ConnectionBanner(),
              const LiveDock(),
              Expanded(
                child: Center(
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(
                      maxWidth: Space.maxContent,
                    ),
                    child: NotificationListener<ScrollNotification>(
                      onNotification: _onScroll,
                      child: _PageFade(page: current.id, child: pages),
                    ),
                  ),
                ),
              ),
            ],
          ),
        );

        return PopScope(
          // Back from any other page goes home first.
          canPop: shown == 0,
          onPopInvokedWithResult: (didPop, _) {
            if (!didPop) _select(home);
          },
          child: ShellScope(
            selected: current.id,
            select: _select,
            available: {for (final tab in tabs) tab.id},
            child: Scaffold(
              body: rail && navigable
                  ? Row(
                      children: [
                        TalvioNavRail(
                          tabs: tabs,
                          shown: shown,
                          onSelect: _select,
                        ),
                        VerticalDivider(
                          width: 1,
                          thickness: 1,
                          color: context.talvio.hairline,
                        ),
                        Expanded(child: body),
                      ],
                    )
                  : body,
              bottomNavigationBar: rail || !navigable
                  ? null
                  : TalvioNavBar(
                      tabs: tabs,
                      shown: shown,
                      onSelect: _select,
                      line: _under && !current.composer,
                    ),
            ),
          ),
        );
      },
    );
  }
}

/// Fades the page in each time [page] changes: a 150 ms crossfade, no
/// movement. Not by re-keying: that would throw away every page's state.
class _PageFade extends StatefulWidget {
  const _PageFade({required this.page, required this.child});

  final String page;
  final Widget child;

  @override
  State<_PageFade> createState() => _PageFadeState();
}

class _PageFadeState extends State<_PageFade>
    with SingleTickerProviderStateMixin {
  late final _c = AnimationController(
    vsync: this,
    duration: Motion.quick,
    value: 1,
  );

  @override
  void didUpdateWidget(_PageFade old) {
    super.didUpdateWidget(old);
    if (old.page != widget.page && motionAllowed(context)) {
      unawaited(_c.forward(from: 0));
    }
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FadeTransition(
    opacity: CurvedAnimation(parent: _c, curve: Motion.enter),
    child: widget.child,
  );
}

/// The bottom navigation (§3.2): the canvas colour, four items, the
/// selected one a filled glyph and an ink label. No pill, no accent.
class TalvioNavBar extends StatelessWidget {
  const TalvioNavBar({
    super.key,
    required this.tabs,
    required this.shown,
    required this.onSelect,
    this.line = false,
  });

  final List<FeatureTab> tabs;
  final int shown;
  final ValueChanged<String> onSelect;

  /// Content scrolls under the bar: a hairline along its top.
  final bool line;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return AnimatedContainer(
      duration: Motion.quick,
      decoration: BoxDecoration(
        color: t.canvas,
        border: Border(top: BorderSide(color: line ? t.hairline : t.canvas)),
      ),
      child: SafeArea(
        top: false,
        child: MediaQuery.withClampedTextScaling(
          maxScaleFactor: 1.15,
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 56),
            child: Row(
              children: [
                for (final (i, tab) in tabs.indexed)
                  Expanded(
                    child: _NavItem(
                      tab: tab,
                      selected: i == shown,
                      onTap: () => onSelect(tab.id),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _NavItem extends StatefulWidget {
  const _NavItem({
    required this.tab,
    required this.selected,
    required this.onTap,
  });

  final FeatureTab tab;
  final bool selected;
  final VoidCallback onTap;

  @override
  State<_NavItem> createState() => _NavItemState();
}

class _NavItemState extends State<_NavItem> {
  var _down = false;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final selected = widget.selected;
    final color = selected ? t.ink : t.muted;
    return Semantics(
      button: true,
      selected: selected,
      label: widget.tab.label,
      onTap: widget.onTap,
      excludeSemantics: true,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: (_) => setState(() => _down = true),
        onTapUp: (_) => setState(() => _down = false),
        onTapCancel: () => setState(() => _down = false),
        onTap: widget.onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              AnimatedScale(
                scale: _down && motionAllowed(context) ? 0.92 : 1,
                duration: Motion.press,
                child: Icon(
                  selected ? widget.tab.selectedIcon : widget.tab.icon,
                  size: IconSize.large,
                  color: color,
                ),
              ),
              const SizedBox(height: Space.s4),
              Text(
                widget.tab.label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TalvioType.caption.copyWith(
                  color: color,
                  fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class TalvioNavRail extends StatelessWidget {
  const TalvioNavRail({
    super.key,
    required this.tabs,
    required this.shown,
    required this.onSelect,
  });

  final List<FeatureTab> tabs;
  final int shown;
  final ValueChanged<String> onSelect;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    // A short window (a phone on its side) scrolls the rail.
    return ColoredBox(
      color: t.canvas,
      child: SafeArea(
        right: false,
        child: SizedBox(
          width: 72,
          child: SingleChildScrollView(
            child: Column(
              children: [
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: Space.s16),
                  child: TalvioMark(size: 24),
                ),
                for (final (i, tab) in tabs.indexed)
                  SizedBox(
                    height: 64,
                    child: _NavItem(
                      tab: tab,
                      selected: i == shown,
                      onTap: () {
                        if (i != shown) onSelect(tab.id);
                      },
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// The top bar (§3.1): the computer (and its switcher), the page's own
/// buttons, the live view and the account.
class _Header extends StatelessWidget {
  const _Header({required this.actions, required this.scrolled});

  final List<Widget> actions;

  /// The page has scrolled under the bar: a hairline along its bottom.
  final bool scrolled;

  @override
  Widget build(BuildContext context) {
    final services = AppServices.of(context);
    final controller = DictateScope.of(context);
    final relay = services.relay;
    final t = context.talvio;
    return ListenableBuilder(
      listenable: Listenable.merge([
        controller,
        relay.devices,
        relay.status,
        services.session,
      ]),
      builder: (context, _) {
        final computer = controller.computer;
        final connected = relay.status.value == RelayStatus.connected;
        return AnimatedContainer(
          duration: Motion.quick,
          decoration: BoxDecoration(
            color: t.canvas,
            border: Border(
              bottom: BorderSide(color: scrolled ? t.hairline : t.canvas),
            ),
          ),
          padding: const EdgeInsetsDirectional.only(start: 8, end: 8),
          constraints: const BoxConstraints(minHeight: 52),
          child: Row(
            children: [
              Expanded(
                child: Align(
                  alignment: AlignmentDirectional.centerStart,
                  child: computer == null
                      ? const Padding(
                          padding: EdgeInsetsDirectional.only(start: 8),
                          child: LogoLockup(size: 24),
                        )
                      : ComputerSwitcher(
                          computer: computer,
                          connecting: !connected,
                          // A recording or request keeps its computer.
                          onTap: canChooseComputer(controller)
                              ? () => chooseComputer(context, controller)
                              : null,
                        ),
                ),
              ),
              if (computer != null) ...actions,
              if (computer != null && connected && canWatch(computer))
                const _LiveViewButton(),
              const _AccountButton(),
            ],
          ),
        );
      },
    );
  }
}

/// Shows the computer's screen in the small view, or hides it again.
class _LiveViewButton extends StatefulWidget {
  const _LiveViewButton();

  @override
  State<_LiveViewButton> createState() => _LiveViewButtonState();
}

class _LiveViewButtonState extends State<_LiveViewButton> {
  var _starting = false;

  Future<void> _start() async {
    setState(() => _starting = true);
    final error = await watchMainScreen(context);
    if (!mounted) return;
    setState(() => _starting = false);
    if (error != null) {
      final name = DictateScope.of(context).computer?.name ?? '';
      showSnack(context, ScreenStrings.error(error, name));
    }
  }

  @override
  Widget build(BuildContext context) {
    final live = LiveScope.maybeOf(context);
    if (live == null) return const SizedBox.shrink();
    final t = context.talvio;
    return ListenableBuilder(
      listenable: live,
      builder: (context, _) {
        final watching = live.watch != null;
        if (_starting) {
          return SizedBox.square(
            dimension: 44,
            child: Center(child: Spinner(color: t.ink)),
          );
        }
        return Stack(
          children: [
            IconBtn(
              icon: watching ? AppIcons.liveViewOn : AppIcons.liveView,
              size: IconSize.large,
              tooltip: watching
                  ? ScreenStrings.hideScreen
                  : ScreenStrings.showScreen,
              selected: watching,
              onPressed: () {
                Haptic.tick();
                watching ? live.close() : unawaited(_start());
              },
            ),
            if (watching)
              PositionedDirectional(
                top: 9,
                end: 9,
                child: IgnorePointer(
                  child: Container(
                    width: 8,
                    height: 8,
                    decoration: BoxDecoration(
                      color: t.live,
                      shape: BoxShape.circle,
                      border: Border.all(color: t.canvas, width: 1.5),
                    ),
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}

class _AccountButton extends StatelessWidget {
  const _AccountButton();

  @override
  Widget build(BuildContext context) {
    final user = AppServices.of(context).session.session?.user;
    final initial = switch (user) {
      final user? =>
        (user.name?.trim().isNotEmpty == true ? user.name! : user.email)
            .characters
            .first
            .toUpperCase(),
      null => '?',
    };
    final t = context.talvio;
    return Tooltip(
      message: ShellStrings.account,
      excludeFromSemantics: true,
      child: Semantics(
        button: true,
        label: ShellStrings.account,
        excludeSemantics: true,
        onTap: () => _open(context),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => _open(context),
          child: SizedBox.square(
            dimension: 44,
            child: Center(
              child: Pressable(
                child: Container(
                  width: 32,
                  height: 32,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    color: t.fill,
                    shape: BoxShape.circle,
                  ),
                  child: Text(
                    initial,
                    style: TalvioType.label.copyWith(
                      color: t.ink,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  static void _open(BuildContext context) {
    Haptic.tick();
    unawaited(
      Navigator.of(
        context,
      ).push(MaterialPageRoute<void>(builder: (_) => const AccountScreen())),
    );
  }
}

/// Under the top bar while the server cannot be reached.
class _ConnectionBanner extends StatelessWidget {
  const _ConnectionBanner();

  @override
  Widget build(BuildContext context) {
    final relay = AppServices.of(context).relay;
    return ListenableBuilder(
      listenable: relay.status,
      builder: (context, _) {
        final down = relay.status.value != RelayStatus.connected;
        return AnimatedSize(
          duration: Motion.standard,
          curve: Motion.enter,
          alignment: Alignment.topCenter,
          child: !down
              ? const SizedBox(width: double.infinity)
              : const Padding(
                  padding: EdgeInsets.only(bottom: Space.s8),
                  child: InlineBanner(
                    tone: Tone.warning,
                    busy: true,
                    message: ShellStrings.reconnecting,
                  ),
                ),
        );
      },
    );
  }
}
