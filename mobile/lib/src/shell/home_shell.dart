import 'package:flutter/material.dart';

import '../core/app_services.dart';
import 'feature_tab.dart';

/// The signed-in app: one tab per available feature.
class HomeShell extends StatefulWidget {
  const HomeShell({super.key, required this.tabs});

  final List<FeatureTab> tabs;

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  // By id, not index: tabs come and go as computers connect.
  late String _selected = widget.tabs.first.id;

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
        return Scaffold(
          body: IndexedStack(
            index: shown,
            children: [
              for (final tab in tabs)
                KeyedSubtree(
                  key: ValueKey(tab.id),
                  child: Builder(builder: tab.builder),
                ),
            ],
          ),
          // NavigationBar needs two destinations.
          bottomNavigationBar: tabs.length < 2
              ? null
              : NavigationBar(
                  selectedIndex: shown,
                  onDestinationSelected: (i) =>
                      setState(() => _selected = tabs[i].id),
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
      },
    );
  }
}
