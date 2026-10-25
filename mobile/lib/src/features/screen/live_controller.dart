import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_scope.dart';
import 'live_protocol.dart';
import 'rtc_link.dart';
import 'watch_controller.dart';
import 'webrtc_link.dart';

/// The WebRTC peer every new [LiveController] uses when none is given; tests
/// put a fake here.
@visibleForTesting
RtcLinkFactory? debugLinkFactory;

/// The one live view of the signed-in app. A watch runs in the small view
/// at the top of every page (the dock), so the computer can be watched
/// while it is told what to do; the full-screen viewer shows the same
/// watch, never a second one.
class LiveController extends ChangeNotifier {
  LiveController({
    required this._relay,
    required this._dictate,
    RtcLinkFactory? linkFactory,
  }) : _linkFactory = linkFactory ?? debugLinkFactory ?? WebRtcLink.create {
    _dictate.addListener(_onDictate);
    _lifecycle = AppLifecycleListener(
      onResume: () {
        _background?.cancel();
        _background = null;
        _watch?.setForeground(true);
      },
      onPause: () {
        _watch?.setForeground(false);
        // A quick look at another app keeps the view; a phone put away
        // stops paying for a picture nobody sees.
        _background?.cancel();
        _background = Timer(backgroundLimit, stop);
      },
    );
  }

  /// How long a view keeps running while the app is in the background.
  static const backgroundLimit = Duration(minutes: 2);

  final RelayClient _relay;
  final DictateController _dictate;
  final RtcLinkFactory _linkFactory;
  late final AppLifecycleListener _lifecycle;
  Timer? _background;

  WatchController? _watch;
  var _expanded = true;
  var _fullScreen = false;
  var _awake = false;
  var _disposed = false;

  /// The watch on screen (running or ended); null when there is none.
  WatchController? get watch => _watch;

  /// The dock shows the picture; collapsed, a one-line bar.
  bool get expanded => _expanded;

  /// The full-screen viewer is open: the dock leaves the picture to it.
  bool get fullScreen => _fullScreen;

  /// Watching [target] on [computer]; a running watch ends first.
  void start(Device computer, WatchTarget target) {
    if (_disposed) return;
    _drop();
    final watch = WatchController(
      relay: _relay,
      computer: computer,
      target: target,
      linkFactory: _linkFactory,
    )..addListener(_onWatch);
    _watch = watch;
    _expanded = true;
    watch.start();
    _notify();
  }

  /// Ends the watch; the dock then says it ended and offers to watch again.
  void stop() => _watch?.stop();

  /// Ends the watch, if it runs, and takes the dock away.
  void close() {
    _drop();
    _notify();
  }

  void setExpanded(bool expanded) {
    if (expanded == _expanded) return;
    _expanded = expanded;
    _notify();
  }

  void setFullScreen(bool fullScreen) {
    if (fullScreen == _fullScreen) return;
    _fullScreen = fullScreen;
    _notify();
  }

  /// Another computer was chosen: a view of the old one goes.
  void _onDictate() {
    final watch = _watch;
    if (watch != null && _dictate.computer?.id != watch.computer.id) close();
  }

  void _onWatch() {
    _keepAwake(_watch?.phase != WatchPhase.ended && _watch != null);
    _notify();
  }

  void _drop() {
    final watch = _watch;
    _watch = null;
    if (watch != null) {
      watch
        ..removeListener(_onWatch)
        ..dispose();
    }
    _keepAwake(false);
  }

  /// The screen stays on while a view runs. Best effort: without the plugin
  /// (tests) it simply does not.
  void _keepAwake(bool on) {
    if (on == _awake) return;
    _awake = on;
    unawaited(
      Future.sync(
        () => on ? WakelockPlus.enable() : WakelockPlus.disable(),
      ).catchError((Object _) {}),
    );
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _background?.cancel();
    _dictate.removeListener(_onDictate);
    _lifecycle.dispose();
    _drop();
    super.dispose();
  }
}

/// The [LiveController] of the signed-in app, below [DictateScope].
class LiveScope extends StatefulWidget {
  const LiveScope({super.key, required this.child, this.linkFactory});

  final Widget child;

  /// For tests: the WebRTC peer to use.
  final RtcLinkFactory? linkFactory;

  static LiveController of(BuildContext context) => maybeOf(context)!;

  static LiveController? maybeOf(BuildContext context) =>
      context.getInheritedWidgetOfExactType<_LiveScopeData>()?.controller;

  @override
  State<LiveScope> createState() => _LiveScopeState();
}

class _LiveScopeState extends State<LiveScope> {
  late final LiveController _controller = LiveController(
    relay: AppServices.of(context).relay,
    dictate: DictateScope.of(context),
    linkFactory: widget.linkFactory,
  );

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      _LiveScopeData(controller: _controller, child: widget.child);
}

class _LiveScopeData extends InheritedWidget {
  const _LiveScopeData({required this.controller, required super.child});

  final LiveController controller;

  @override
  bool updateShouldNotify(_LiveScopeData oldWidget) =>
      controller != oldWidget.controller;
}
