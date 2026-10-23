import 'package:flutter/material.dart';

import '../core/models.dart';

/// One page of the home shell, as its menu lists it. The first is home.
class FeatureTab {
  const FeatureTab({
    required this.id,
    required this.label,
    required this.icon,
    required this.selectedIcon,
    required this.builder,
    this.requires,
    this.secondary = false,
  });

  final String id;
  final String label;
  final IconData icon;
  final IconData selectedIcon;
  final WidgetBuilder builder;

  /// The capability a computer must advertise for this tab to show; null for
  /// a tab that is always there.
  final String? requires;

  /// Listed below a divider in the menu, away from the things you do (the
  /// account, settings).
  final bool secondary;

  /// Hidden only when computers are online and none of them advertises
  /// [requires]. A computer that advertises nothing is unknown, not lacking:
  /// offline, or connected but not yet past its `hello`. Hiding the tab for
  /// that moment of every reconnect would throw away the tab's state (a
  /// recording, a kept retry, typed text).
  bool availableWith(List<Device> devices) {
    final needed = requires;
    if (needed == null) return true;
    final online = devices.where((d) => d.isComputer && d.online);
    return online.isEmpty ||
        online.any(
          (d) => d.capabilities.isEmpty || d.capabilities.contains(needed),
        );
  }
}
