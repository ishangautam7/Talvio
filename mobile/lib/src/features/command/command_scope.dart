import 'package:flutter/widgets.dart';

import '../../core/app_services.dart';
import '../dictate/dictate_scope.dart';
import 'command_controller.dart';

/// The one [CommandController] of the signed-in app, below [DictateScope]
/// (whose computer it sends to). It lives as long as the signed-in shell, so
/// a command waiting for Do it survives the Do page coming and going.
class CommandScope extends StatefulWidget {
  const CommandScope({super.key, required this.child});

  final Widget child;

  static CommandController of(BuildContext context) =>
      context.getInheritedWidgetOfExactType<_CommandScopeData>()!.controller;

  @override
  State<CommandScope> createState() => _CommandScopeState();
}

class _CommandScopeState extends State<CommandScope> {
  late final AppServices _services = AppServices.of(context);
  late final CommandController _controller = CommandController(
    relay: _services.relay,
    prefs: _services.prefs,
    dictate: DictateScope.of(context),
  );

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      _CommandScopeData(controller: _controller, child: widget.child);
}

class _CommandScopeData extends InheritedWidget {
  const _CommandScopeData({required this.controller, required super.child});

  final CommandController controller;

  @override
  bool updateShouldNotify(_CommandScopeData oldWidget) =>
      controller != oldWidget.controller;
}
