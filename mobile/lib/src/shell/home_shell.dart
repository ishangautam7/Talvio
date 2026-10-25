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

  void _select(String id) {
    if (id == _selected) return;
    Haptic.tick();
    FocusManager.instance.primaryFocus?.unfocus();
    setState(() => _selected = id);
  }

  @override
  Widget build(BuildContext context) {
    final relay = AppServices.of(context).relay;
    return ListenableBuilder(
      listenable: relay.devices,
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
              _Header(actions: current.actions?.call(context) ?? const []),
              const _ConnectionStrip(),
              const LiveDock(),
              Expanded(
                child: _PageFade(page: current.id, child: pages),
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
              body: rail
                  ? Row(
                      children: [
                        _Rail(tabs: tabs, shown: shown, onSelect: _select),
                        VerticalDivider(
                          width: 1,
                          color: context.talvio.hairline,
                        ),
                        Expanded(child: body),
                      ],
                    )
                  : body,
              bottomNavigationBar: rail || tabs.length < 2
                  ? null
                  : _BottomBar(tabs: tabs, shown: shown, onSelect: _select),
            ),
          ),
        );
      },
    );
  }
}

/// Fades the page in, lifting it a little, each time [page] changes. Not
/// by re-keying: that would throw away every page's state.
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
    duration: Motion.medium,
    value: 1,
  );

  @override
  void didUpdateWidget(_PageFade old) {
    super.didUpdateWidget(old);
    if (old.page != widget.page) _c.forward(from: 0);
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, child) {
      final t = Motion.emphasized.transform(_c.value);
      return Opacity(
        opacity: 0.4 + 0.6 * t,
        child: Transform.translate(
          offset: Offset(0, 8 * (1 - t)),
          child: child,
        ),
      );
    },
    child: widget.child,
  );
}

class _BottomBar extends StatelessWidget {
  const _BottomBar({
    required this.tabs,
    required this.shown,
    required this.onSelect,
  });

  final List<FeatureTab> tabs;
  final int shown;
  final ValueChanged<String> onSelect;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: context.talvio.hairline)),
      ),
      child: NavigationBar(
        selectedIndex: shown,
        onDestinationSelected: (i) => onSelect(tabs[i].id),
        destinations: [
          for (final tab in tabs)
            NavigationDestination(
              icon: Icon(tab.icon),
              selectedIcon: Icon(tab.selectedIcon),
              label: tab.label,
            ),
        ],
      ),
    );
  }
}

class _Rail extends StatelessWidget {
  const _Rail({
    required this.tabs,
    required this.shown,
    required this.onSelect,
  });

  final List<FeatureTab> tabs;
  final int shown;
  final ValueChanged<String> onSelect;

  @override
  Widget build(BuildContext context) {
    // A short window (a phone on its side) scrolls the rail.
    return LayoutBuilder(
      builder: (context, constraints) => SingleChildScrollView(
        child: ConstrainedBox(
          constraints: BoxConstraints(minHeight: constraints.maxHeight),
          child: IntrinsicHeight(
            child: NavigationRail(
              selectedIndex: shown,
              labelType: NavigationRailLabelType.all,
              onDestinationSelected: (i) => onSelect(tabs[i].id),
              leading: const SafeArea(
                bottom: false,
                child: Padding(
                  padding: EdgeInsets.symmetric(vertical: Space.md),
                  child: TalvioMark(size: 32),
                ),
              ),
              destinations: [
                for (final tab in tabs)
                  NavigationRailDestination(
                    icon: Icon(tab.icon),
                    selectedIcon: Icon(tab.selectedIcon),
                    label: Text(tab.label),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// The top bar: the computer (and its switcher), the page's own buttons,
/// the screen and the account.
class _Header extends StatelessWidget {
  const _Header({required this.actions});

  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final services = AppServices.of(context);
    final controller = DictateScope.of(context);
    final relay = services.relay;
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
        return Padding(
          padding: const EdgeInsets.fromLTRB(
            Space.md,
            Space.sm,
            Space.sm,
            Space.sm,
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 48),
            child: Row(
              children: [
                Expanded(
                  child: Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: computer == null
                        ? const Padding(
                            padding: EdgeInsets.only(left: Space.xs),
                            child: LogoLockup(size: 26),
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
                  const _ScreenButton(),
                const _AccountButton(),
              ],
            ),
          ),
        );
      },
    );
  }
}

/// Shows the computer's screen in the small view, or hides it again.
class _ScreenButton extends StatefulWidget {
  const _ScreenButton();

  @override
  State<_ScreenButton> createState() => _ScreenButtonState();
}

class _ScreenButtonState extends State<_ScreenButton> {
  var _starting = false;

  Future<void> _start() async {
    setState(() => _starting = true);
    final error = await watchMainScreen(context);
    if (!mounted) return;
    setState(() => _starting = false);
    if (error != null) {
      final name = DictateScope.of(context).computer?.name ?? '';
      ScaffoldMessenger.of(context)
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(content: Text(ScreenStrings.error(error, name))),
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final live = LiveScope.maybeOf(context);
    if (live == null) return const SizedBox.shrink();
    return ListenableBuilder(
      listenable: live,
      builder: (context, _) {
        final watching = live.watch != null;
        if (_starting) {
          return const Padding(
            padding: EdgeInsets.all(14),
            child: ButtonSpinner(),
          );
        }
        return IconButton(
          tooltip: watching
              ? ScreenStrings.hideScreen
              : ScreenStrings.showScreen,
          isSelected: watching,
          onPressed: watching ? live.close : _start,
          icon: const Icon(Icons.cast_rounded),
          selectedIcon: Icon(
            Icons.cast_connected_rounded,
            color: context.talvio.accent,
          ),
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
      child: Semantics(
        button: true,
        label: ShellStrings.account,
        excludeSemantics: true,
        onTap: () => _open(context),
        child: InkResponse(
          onTap: () => _open(context),
          radius: 24,
          child: Padding(
            padding: const EdgeInsets.all(6),
            child: Container(
              width: 34,
              height: 34,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: t.accentSoft,
                shape: BoxShape.circle,
                border: Border.all(color: t.hairline),
              ),
              child: Text(
                initial,
                style: context.text.labelLarge?.copyWith(color: t.accent),
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

/// A thin line under the top bar while the server cannot be reached.
class _ConnectionStrip extends StatelessWidget {
  const _ConnectionStrip();

  @override
  Widget build(BuildContext context) {
    final relay = AppServices.of(context).relay;
    final t = context.talvio;
    return ListenableBuilder(
      listenable: relay.status,
      builder: (context, _) {
        final down = relay.status.value != RelayStatus.connected;
        return AnimatedSize(
          duration: Motion.medium,
          curve: Motion.emphasized,
          child: !down
              ? const SizedBox(width: double.infinity)
              : Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.lg,
                    0,
                    Space.lg,
                    Space.sm,
                  ),
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      color: t.warningSoft,
                      borderRadius: BorderRadius.circular(Radii.md),
                    ),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: Space.md,
                        vertical: Space.sm,
                      ),
                      child: Row(
                        children: [
                          SizedBox.square(
                            dimension: 14,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: t.warning,
                            ),
                          ),
                          const SizedBox(width: Space.sm),
                          Expanded(
                            child: Semantics(
                              liveRegion: true,
                              child: Text(
                                ShellStrings.reconnecting,
                                style: context.text.labelMedium?.copyWith(
                                  color: t.warning,
                                ),
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
        );
      },
    );
  }
}
