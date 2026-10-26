import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

/// What a page can ask of the home shell: go to another page.
class ShellScope extends InheritedWidget {
  const ShellScope({
    super.key,
    required this.selected,
    required this.select,
    required this.available,
    required super.child,
  });

  /// The id of the page on screen.
  final String selected;

  /// Shows the page [id] (a [FeatureTab.id]).
  final ValueChanged<String> select;

  /// The ids of the pages available now.
  final Set<String> available;

  static ShellScope? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<ShellScope>();

  @override
  bool updateShouldNotify(ShellScope oldWidget) =>
      selected != oldWidget.selected ||
      !setEquals(available, oldWidget.available);
}
