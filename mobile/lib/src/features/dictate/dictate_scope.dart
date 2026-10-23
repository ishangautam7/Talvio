import 'package:flutter/widgets.dart';

import '../../core/app_services.dart';
import 'dictate_controller.dart';

/// The one [DictateController] of the signed-in app. Home, Send and History
/// all show the same computer, window, recording and request, so they share
/// it; it lives as long as the signed-in shell.
class DictateScope extends StatefulWidget {
  const DictateScope({super.key, required this.child});

  final Widget child;

  static DictateController of(BuildContext context) =>
      context.getInheritedWidgetOfExactType<_DictateScopeData>()!.controller;

  @override
  State<DictateScope> createState() => _DictateScopeState();
}

class _DictateScopeState extends State<DictateScope> {
  late final AppServices _services = AppServices.of(context);
  late final DictateController _controller = DictateController(
    relay: _services.relay,
    prefs: _services.prefs,
  );

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      _DictateScopeData(controller: _controller, child: widget.child);
}

class _DictateScopeData extends InheritedWidget {
  const _DictateScopeData({required this.controller, required super.child});

  final DictateController controller;

  @override
  bool updateShouldNotify(_DictateScopeData oldWidget) =>
      controller != oldWidget.controller;
}
