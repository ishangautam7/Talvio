import 'package:flutter/widgets.dart';

import '../../core/app_services.dart';
import '../command/command_scope.dart';
import '../dictate/dictate_scope.dart';
import 'assistant_controller.dart';

/// The one [AssistantController] of the signed-in app, below [DictateScope]
/// and [CommandScope]. It lives as long as the signed-in shell, so the chat
/// survives the Assistant page coming and going, and live view (a route
/// outside the shell) is handed the same one.
class AssistantScope extends StatefulWidget {
  const AssistantScope({super.key, required this.child});

  final Widget child;

  static AssistantController of(BuildContext context) => maybeOf(context)!;

  static AssistantController? maybeOf(BuildContext context) =>
      context.getInheritedWidgetOfExactType<_AssistantScopeData>()?.controller;

  @override
  State<AssistantScope> createState() => _AssistantScopeState();
}

class _AssistantScopeState extends State<AssistantScope> {
  late final AppServices _services = AppServices.of(context);
  late final AssistantController _controller = AssistantController(
    relay: _services.relay,
    prefs: _services.prefs,
    dictate: DictateScope.of(context),
    command: CommandScope.of(context),
  );

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      _AssistantScopeData(controller: _controller, child: widget.child);
}

class _AssistantScopeData extends InheritedWidget {
  const _AssistantScopeData({required this.controller, required super.child});

  final AssistantController controller;

  @override
  bool updateShouldNotify(_AssistantScopeData oldWidget) =>
      controller != oldWidget.controller;
}
