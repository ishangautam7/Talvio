import 'package:flutter/material.dart';

import '../core/app_services.dart';
import 'feature_tab.dart';
import 'shell_scope.dart';

/// The signed-in app: one page per available feature, chosen from a menu
/// that slides in from the side (the menu button, or a swipe from the left
/// edge). A wide window (a tablet, a phone on its side) keeps the pages in a
/// rail instead.
class HomeShell extends StatefulWidget {
  const HomeShell({super.key, required this.tabs, this.header});

  /// The first is home.
  final List<FeatureTab> tabs;

  /// The top of the menu (who is signed in, which computer).
  final WidgetBuilder? header;

  /// From this width the menu is a rail beside the page.
  static const railWidth = 640.0;

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  final _scaffold = GlobalKey<ScaffoldState>();

  // By id, not index: pages come and go as computers connect.
  late String _selected = widget.tabs.first.id;

  void _select(String id) {
    if (id != _selected) setState(() => _selected = id);
  }

  void _openMenu() => _scaffold.currentState?.openDrawer();

  @override
  Widget build(BuildContext context) {
    final devices = AppServices.of(context).relay.devices;
    return ListenableBuilder(
      listenable: devices,
      builder: (context, _) {
        final tabs = [
          for (final tab in widget.tabs)
            if (tab.availableWith(devices.value ?? const [])) tab,
        ];
        final index = tabs.indexWhere((tab) => tab.id == _selected);
        final shown = index < 0 ? 0 : index;
        final home = tabs.first.id;
        final rail = MediaQuery.sizeOf(context).width >= HomeShell.railWidth;

        // Every page keeps its place in the stack, available or not:
        // IndexedStack matches its children by position, so a page coming
        // or going in front of another would throw that one's state away
        // (a recording, typed text).
        final pages = IndexedStack(
          index: widget.tabs.indexWhere((tab) => tab.id == tabs[shown].id),
          children: [
            for (final tab in widget.tabs)
              KeyedSubtree(
                key: ValueKey(tab.id),
                child: Builder(builder: tab.builder),
              ),
          ],
        );

        return PopScope(
          // Back from any other page goes home first.
          canPop: shown == 0,
          onPopInvokedWithResult: (didPop, _) {
            if (!didPop) _select(home);
          },
          child: ShellScope(
            selected: tabs[shown].id,
            select: _select,
            openMenu: rail ? null : _openMenu,
            available: {for (final tab in tabs) tab.id},
            child: Scaffold(
              key: _scaffold,
              drawer: rail
                  ? null
                  : _Menu(
                      tabs: tabs,
                      shown: shown,
                      header: widget.header,
                      onSelect: (id) {
                        _scaffold.currentState?.closeDrawer();
                        _select(id);
                      },
                    ),
              body: rail
                  ? Row(
                      children: [
                        _Rail(tabs: tabs, shown: shown, onSelect: _select),
                        const VerticalDivider(width: 1),
                        Expanded(child: pages),
                      ],
                    )
                  : pages,
            ),
          ),
        );
      },
    );
  }
}

class _Menu extends StatelessWidget {
  const _Menu({
    required this.tabs,
    required this.shown,
    required this.header,
    required this.onSelect,
  });

  final List<FeatureTab> tabs;
  final int shown;
  final WidgetBuilder? header;
  final ValueChanged<String> onSelect;

  @override
  Widget build(BuildContext context) {
    // NavigationDrawer numbers its destinations in the order they appear.
    final primary = [
      for (final tab in tabs)
        if (!tab.secondary) tab,
    ];
    final secondary = [
      for (final tab in tabs)
        if (tab.secondary) tab,
    ];
    final ordered = [...primary, ...secondary];
    final header = this.header;
    return NavigationDrawer(
      selectedIndex: ordered.indexOf(tabs[shown]),
      onDestinationSelected: (i) => onSelect(ordered[i].id),
      children: [
        if (header != null) Builder(builder: header),
        for (final tab in primary) _destination(tab),
        if (secondary.isNotEmpty) ...[
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: 28, vertical: 8),
            child: Divider(),
          ),
          for (final tab in secondary) _destination(tab),
        ],
      ],
    );
  }

  static NavigationDrawerDestination _destination(FeatureTab tab) =>
      NavigationDrawerDestination(
        icon: Icon(tab.icon),
        selectedIcon: Icon(tab.selectedIcon),
        label: Text(tab.label),
      );
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
