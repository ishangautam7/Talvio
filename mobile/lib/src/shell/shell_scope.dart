import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'strings.dart';

/// What a page can ask of the home shell: open the menu, or go to another
/// page.
class ShellScope extends InheritedWidget {
  const ShellScope({
    super.key,
    required this.selected,
    required this.select,
    required this.openMenu,
    required this.available,
    required super.child,
  });

  /// The id of the page on screen.
  final String selected;

  /// Shows the page [id] (a [FeatureTab.id]).
  final ValueChanged<String> select;

  /// Opens the menu; null where the menu is always on screen (a rail).
  final VoidCallback? openMenu;

  /// The ids of the pages available now.
  final Set<String> available;

  static ShellScope? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<ShellScope>();

  @override
  bool updateShouldNotify(ShellScope oldWidget) =>
      selected != oldWidget.selected ||
      (openMenu == null) != (oldWidget.openMenu == null) ||
      !setEquals(available, oldWidget.available);
}

/// The menu button for a page's app bar; null when there is no menu to open
/// (a wide screen shows it as a rail, and a page outside the shell has none).
Widget? shellLeading(BuildContext context) {
  final open = ShellScope.maybeOf(context)?.openMenu;
  if (open == null) return null;
  return IconButton(
    tooltip: ShellStrings.openMenu,
    onPressed: open,
    icon: const Icon(Icons.menu),
  );
}
