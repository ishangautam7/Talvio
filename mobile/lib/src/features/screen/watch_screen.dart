import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../theme.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/dictate_screen.dart' show typedTextLimit;
import '../dictate/key_pad.dart';
import '../dictate/remote_windows.dart';
import '../dictate/strings.dart';
import 'live_protocol.dart';
import 'rtc_link.dart';
import 'strings.dart';
import 'view_geometry.dart';
import 'watch_controller.dart';
import 'webrtc_link.dart';

/// A screen as the picker and the viewer name it.
String screenLabel(RemoteScreen screen) =>
    screen.name.isEmpty ? ScreenStrings.untitledScreen : screen.name;

/// The route of live view. [dictate] is the Dictate tab's controller, whose
/// chosen window is already the watched one, so the mic here records and
/// sends exactly as that tab does.
Route<void> watchRoute({
  required DictateController dictate,
  required Device computer,
  required WatchTarget target,
  RtcLinkFactory? linkFactory,
}) => MaterialPageRoute(
  builder: (_) => WatchScreen(
    dictate: dictate,
    computer: computer,
    target: target,
    linkFactory: linkFactory ?? WebRtcLink.create,
  ),
);

/// Opens live view of [target] on [dictate]'s computer. What is watched is
/// also where dictation and typing go: [window], or for a screen wherever
/// the keyboard is. Completes when the viewer closes.
Future<void> openWatch(
  BuildContext context,
  DictateController dictate,
  WatchTarget target, {
  RemoteWindow? window,
}) async {
  final computer = dictate.computer;
  if (computer == null) return;
  dictate.chooseWindow(window);
  await Navigator.of(
    context,
  ).push(watchRoute(dictate: dictate, computer: computer, target: target));
}

class WatchScreen extends StatefulWidget {
  const WatchScreen({
    super.key,
    required this.dictate,
    required this.computer,
    required this.target,
    required this.linkFactory,
  });

  final DictateController dictate;
  final Device computer;
  final WatchTarget target;
  final RtcLinkFactory linkFactory;

  @override
  State<WatchScreen> createState() => _WatchScreenState();
}

class _WatchScreenState extends State<WatchScreen>
    with SingleTickerProviderStateMixin {
  late final WatchController _watch;
  late final AppLifecycleListener _lifecycle;
  final _transform = TransformationController();
  late final AnimationController _move = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 280),
  );
  Animation<Matrix4>? _moveTo;
  final _text = TextEditingController();
  final _textFocus = FocusNode();

  /// Keep the text cursor in view. Any pan or zoom by hand turns it off.
  var _follow = true;
  var _typing = false;

  /// The key pad is open (closed at first, so the picture gets the room).
  var _keys = false;

  /// The picture fills the view, cropped, rather than fits in it whole.
  var _filled = false;

  /// The orientation the viewer is held in; null to follow the phone.
  Orientation? _held;
  Size _viewport = Size.zero;
  Caret? _followed;
  DictateOutcome? _shownOutcome;

  // One pan, zoom or two-finger scroll in progress.
  Matrix4? _gestureFrom;
  Offset _gestureFocal = Offset.zero;
  bool? _gestureScrolls;
  int _scrolled = 0;
  Offset? _scrollAt;
  Offset? _doubleTapAt;

  @override
  void initState() {
    super.initState();
    _watch = WatchController(
      relay: AppServices.of(context).relay,
      computer: widget.computer,
      target: widget.target,
      linkFactory: widget.linkFactory,
    )..addListener(_onWatch);
    widget.dictate.addListener(_onDictate);
    _shownOutcome = widget.dictate.outcome;
    _move.addListener(() {
      if (_moveTo case final to?) _transform.value = to.value;
    });
    _lifecycle = AppLifecycleListener(
      onResume: () => _watch.setForeground(true),
      onPause: () => _watch.setForeground(false),
    );
    _keepAwake(true);
    _watch.start();
  }

  @override
  void dispose() {
    _keepAwake(false);
    if (_held != null) _hold(null);
    _lifecycle.dispose();
    widget.dictate.removeListener(_onDictate);
    _watch
      ..removeListener(_onWatch)
      ..dispose();
    _move.dispose();
    _transform.dispose();
    _text.dispose();
    _textFocus.dispose();
    super.dispose();
  }

  /// The screen stays on while watching. Best effort: without the plugin
  /// (tests) it simply does not.
  static void _keepAwake(bool on) => unawaited(
    Future.sync(
      () => on ? WakelockPlus.enable() : WakelockPlus.disable(),
    ).catchError((Object _) {}),
  );

  String get _name => widget.computer.name;

  /// Holds the viewer sideways or upright; null lets the phone decide again.
  /// Best effort, like the wake lock.
  static void _hold(Orientation? orientation) => unawaited(
    Future.sync(
      () => SystemChrome.setPreferredOrientations(switch (orientation) {
        Orientation.landscape => const [
          DeviceOrientation.landscapeLeft,
          DeviceOrientation.landscapeRight,
        ],
        Orientation.portrait => const [DeviceOrientation.portraitUp],
        null => const [],
      }),
    ).catchError((Object _) {}),
  );

  void _rotate(Orientation now) {
    final next = now == Orientation.portrait
        ? Orientation.landscape
        : Orientation.portrait;
    setState(() => _held = next);
    _hold(next);
  }

  /// Fills the view with the picture, or shows all of it again. Either is a
  /// view chosen by hand, so it stops Follow.
  void _toggleFill() {
    final fill = !_filled;
    setState(() {
      _filled = fill;
      _follow = false;
    });
    _moveView(fill ? fillTransform(_picture, _viewport) : Matrix4.identity());
  }

  void _moveView(Matrix4 to) {
    _moveTo = Matrix4Tween(
      begin: _transform.value.clone(),
      end: to,
    ).animate(CurvedAnimation(parent: _move, curve: Curves.easeOutCubic));
    unawaited(_move.forward(from: 0));
  }

  /// The picture's size: the decoded frames', else the window's as the
  /// computer described it, else the viewport's (no letterbox).
  Size get _picture {
    if (_watch.link?.videoSize.value case final size?) return size;
    if (_watch.watched case WatchedTarget(
      :final width,
      :final height,
    ) when width > 0 && height > 0) {
      return Size(width.toDouble(), height.toDouble());
    }
    return _viewport;
  }

  void _onWatch() {
    if (_watch.caret != _followed) {
      _followed = _watch.caret;
      _applyFollow();
    }
  }

  void _onDictate() {
    final outcome = widget.dictate.outcome;
    if (identical(outcome, _shownOutcome)) return;
    _shownOutcome = outcome;
    if (outcome?.error case final error? when mounted) {
      _snack(DictateStrings.error(error, _name));
    }
  }

  /// Moves the view to the caret, if Follow is on and there is one.
  void _applyFollow({bool animate = true}) {
    final caret = _watch.caret;
    if (!_follow || caret == null || _viewport.isEmpty) return;
    final to = followTransform(
      current: _transform.value,
      caret: caret,
      viewport: _viewport,
      picture: _picture,
    );
    if (to == _transform.value) return;
    if (_filled) setState(() => _filled = false);
    if (!animate) {
      _move.stop();
      _transform.value = to;
      return;
    }
    _moveView(to);
  }

  void _setFollow(bool follow) {
    setState(() => _follow = follow);
    if (follow) _applyFollow();
  }

  void _snack(String text) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(text)));
  }

  void _showInputError(InputResult result) {
    if (result.ok || !mounted) return;
    _snack(ScreenStrings.error(result.error!, _name));
  }

  Future<void> _press(List<String> keys) async {
    unawaited(HapticFeedback.lightImpact());
    _showInputError(await _watch.keys(keys));
  }

  Future<void> _sendText() async {
    final text = _text.text.trim();
    if (text.isEmpty) return;
    final result = await _watch.text(text);
    if (!mounted) return;
    if (result.ok) {
      _text.clear();
    } else {
      _showInputError(result);
    }
  }

  void _toggleKeyboard() {
    setState(() => _typing = !_typing);
    if (_typing) {
      _textFocus.requestFocus();
    } else {
      _textFocus.unfocus();
    }
  }

  Future<void> _setControl(bool control) async {
    _snack(control ? ScreenStrings.controlHint : ScreenStrings.viewHint);
    await _watch.setControl(control);
  }

  /// A click at [point] (viewport coordinates), asking first when the
  /// computer says the control sends, buys, posts or deletes.
  Future<void> _click(Offset point, {bool right = false, int count = 1}) async {
    final at = pictureCoords(
      point: point,
      transform: _transform.value,
      viewport: _viewport,
      picture: _picture,
    );
    if (at == null) return;
    unawaited(HapticFeedback.selectionClick());
    var result = await _watch.tap(at, right: right, count: count);
    if (result.needsConfirm) {
      if (!mounted || !await _confirm(result.label)) return;
      result = await _watch.tap(
        at,
        right: right,
        count: count,
        confirmed: true,
      );
    }
    _showInputError(result);
  }

  Future<bool> _confirm(String? label) async {
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(
          label == null || label.isEmpty
              ? ScreenStrings.confirmUnlabelled
              : ScreenStrings.confirmTitle(label),
        ),
        content: const Text(ScreenStrings.confirmBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text(ScreenStrings.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text(ScreenStrings.press),
          ),
        ],
      ),
    );
    return yes ?? false;
  }

  void _onInteractionStart(ScaleStartDetails details) {
    _move.stop();
    _gestureFrom = _transform.value.clone();
    _gestureFocal = details.localFocalPoint;
    _gestureScrolls = null;
    _scrolled = 0;
    _scrollAt = pictureCoords(
      point: details.localFocalPoint,
      transform: _transform.value,
      viewport: _viewport,
      picture: _picture,
    );
  }

  /// In Control, two fingers moving up or down together without pinching
  /// scroll the computer instead of moving the view.
  void _onInteractionUpdate(ScaleUpdateDetails details) {
    final from = _gestureFrom;
    if (!_watch.control || from == null || details.pointerCount < 2) return;
    final moved = details.localFocalPoint - _gestureFocal;
    _gestureScrolls ??= (details.scale - 1).abs() > 0.08
        ? false
        : (moved.dy.abs() > 24 && moved.dy.abs() > moved.dx.abs() * 1.5
              ? true
              : null);
    if (_gestureScrolls != true) return;
    _transform.value = from;
    final notches = scrollNotches(moved.dy) - _scrolled;
    final at = _scrollAt;
    if (notches == 0 || at == null) return;
    _scrolled += notches;
    unawaited(_watch.scroll(at, dy: notches).then(_showInputError));
  }

  void _onInteractionEnd(ScaleEndDetails details) {
    final from = _gestureFrom;
    _gestureFrom = null;
    if (from == null || from == _transform.value) return;
    if (_follow) setState(() => _follow = false);
    final zoomed =
        (from.getMaxScaleOnAxis() - _transform.value.getMaxScaleOnAxis())
            .abs() >
        0.01;
    if (_filled && zoomed) setState(() => _filled = false);
  }

  @override
  Widget build(BuildContext context) {
    return Theme(
      data: buildTheme(Brightness.dark),
      child: Builder(
        builder: (context) => ListenableBuilder(
          listenable: Listenable.merge([_watch, widget.dictate]),
          builder: (context, _) => Scaffold(
            backgroundColor: Colors.black,
            body: MediaQuery.orientationOf(context) == Orientation.landscape
                ? Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(child: _buildStage(context)),
                      _buildSide(context),
                    ],
                  )
                : Column(
                    children: [
                      Expanded(child: _buildStage(context)),
                      _buildBar(context),
                    ],
                  ),
          ),
        ),
      ),
    );
  }

  Widget _buildStage(BuildContext context) {
    final w = _watch;
    final link = w.link;
    final control = w.control && w.phase == WatchPhase.live;
    return Stack(
      children: [
        Positioned.fill(
          child: LayoutBuilder(
            builder: (context, constraints) {
              final viewport = constraints.biggest;
              if (viewport != _viewport) {
                _viewport = viewport;
                // Rotated or resized: put the caret back in view.
                WidgetsBinding.instance.addPostFrameCallback(
                  (_) => _applyFollow(animate: false),
                );
              }
              return GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTapUp: control ? (d) => _click(d.localPosition) : null,
                onDoubleTapDown: control
                    ? (d) => _doubleTapAt = d.localPosition
                    : null,
                onDoubleTap: control
                    ? () {
                        if (_doubleTapAt case final at?) _click(at, count: 2);
                      }
                    : null,
                onLongPressStart: control
                    ? (d) => _click(d.localPosition, right: true)
                    : null,
                child: InteractiveViewer(
                  transformationController: _transform,
                  minScale: 1,
                  maxScale: 8,
                  onInteractionStart: _onInteractionStart,
                  onInteractionUpdate: _onInteractionUpdate,
                  onInteractionEnd: _onInteractionEnd,
                  child: SizedBox.fromSize(
                    size: viewport,
                    child: link == null || !w.foreground
                        ? const ColoredBox(color: Colors.black)
                        : ValueListenableBuilder(
                            valueListenable: link.videoSize,
                            builder: (context, size, child) {
                              // The first frame sets the letterbox.
                              if (size != null) {
                                WidgetsBinding.instance.addPostFrameCallback(
                                  (_) => _applyFollow(animate: false),
                                );
                              }
                              return child!;
                            },
                            child: Semantics(
                              label: ScreenStrings.watchItem(w.title),
                              image: true,
                              child: link.buildVideo(),
                            ),
                          ),
                  ),
                ),
              );
            },
          ),
        ),
        Positioned(
          left: 0,
          right: 0,
          top: 0,
          child: _TopBar(
            title: w.title,
            computerName: _name,
            status: _statusText(),
            live: w.phase == WatchPhase.live && !w.reconnecting,
            follow: _follow,
            onFollow: w.caret == null ? null : () => _setFollow(!_follow),
            onClose: () => Navigator.maybePop(context),
            filled: _filled,
            // Only a picture that leaves bars can fill the view.
            onFill:
                _filled ||
                    (link != null &&
                        !_viewport.isEmpty &&
                        fillScale(_picture, _viewport) >= 1.05)
                ? _toggleFill
                : null,
            sideways:
                MediaQuery.orientationOf(context) == Orientation.landscape,
            onRotate: () => _rotate(MediaQuery.orientationOf(context)),
            notice: switch (w.watched?.state) {
              'minimized' => ScreenStrings.minimized,
              'covered' => ScreenStrings.covered,
              _ => null,
            },
          ),
        ),
        if (w.phase == WatchPhase.ended && !w.restarting)
          Center(child: _endedCard(context))
        else if (w.phase != WatchPhase.live || w.restarting)
          Center(child: _waiting(context)),
      ],
    );
  }

  String _statusText() {
    final w = _watch;
    if (w.restarting) return ScreenStrings.switching;
    return switch (w.phase) {
      WatchPhase.starting => ScreenStrings.starting,
      WatchPhase.approval => ScreenStrings.approval,
      WatchPhase.connecting => ScreenStrings.connecting,
      WatchPhase.live when w.reconnecting => ScreenStrings.reconnecting,
      WatchPhase.live => ScreenStrings.live,
      WatchPhase.ended => ScreenStrings.ended,
    };
  }

  Widget _waiting(BuildContext context) {
    final theme = Theme.of(context);
    final approval = _watch.phase == WatchPhase.approval;
    return Padding(
      padding: const EdgeInsets.all(32),
      child: Semantics(
        liveRegion: true,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const CircularProgressIndicator(),
            const SizedBox(height: 16),
            Text(
              approval ? DictateStrings.approval(_name) : _statusText(),
              textAlign: TextAlign.center,
              style: theme.textTheme.titleMedium,
            ),
            if (approval)
              Text(
                ScreenStrings.approvalDetail,
                textAlign: TextAlign.center,
                style: theme.textTheme.bodyMedium?.copyWith(
                  color: theme.colorScheme.onSurfaceVariant,
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _endedCard(BuildContext context) {
    final theme = Theme.of(context);
    final error = _watch.endError;
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Card.filled(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 8, 8),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Semantics(
                liveRegion: true,
                child: Text(
                  error == null
                      ? ScreenStrings.stoppedByYou
                      : ScreenStrings.error(error, _name),
                  style: theme.textTheme.bodyLarge,
                ),
              ),
              OverflowBar(
                alignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: () => Navigator.maybePop(context),
                    child: const Text(ScreenStrings.close),
                  ),
                  if (error != 'target_gone')
                    TextButton(
                      onPressed: _watch.start,
                      child: const Text(ScreenStrings.watchAgain),
                    ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// The mic: dictates into the watched window, always pasting.
  Widget _mic(BuildContext context) {
    final w = _watch;
    final d = widget.dictate;
    final canDictate =
        w.phase == WatchPhase.live &&
        widget.computer.capabilities.contains(DictateKind.dictate) &&
        !w.restarting;
    return switch (d.phase) {
      DictatePhase.sending => const Padding(
        padding: EdgeInsets.all(14),
        child: SizedBox.square(
          dimension: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      ),
      DictatePhase.recording => IconButton.filled(
        tooltip: ScreenStrings.stopDictating,
        style: IconButton.styleFrom(
          backgroundColor: Theme.of(context).colorScheme.error,
          foregroundColor: Theme.of(context).colorScheme.onError,
        ),
        onPressed: () => d.stopAndSend(deliverAs: Deliver.paste),
        icon: const Icon(Icons.stop_rounded),
      ),
      DictatePhase.idle => IconButton.filledTonal(
        tooltip: ScreenStrings.dictate,
        onPressed: canDictate ? d.startRecording : null,
        icon: const Icon(Icons.mic),
      ),
    };
  }

  /// Mic, typing, the key pad and sound.
  List<Widget> _buttons(BuildContext context) {
    final w = _watch;
    final live = w.phase == WatchPhase.live;
    return [
      _mic(context),
      IconButton(
        tooltip: _typing ? ScreenStrings.hideKeyboard : ScreenStrings.keyboard,
        isSelected: _typing,
        onPressed: live ? _toggleKeyboard : null,
        icon: const Icon(Icons.keyboard_outlined),
        selectedIcon: const Icon(Icons.keyboard_hide_outlined),
      ),
      IconButton(
        tooltip: _keys ? ScreenStrings.hideKeys : ScreenStrings.keys,
        isSelected: _keys,
        onPressed: () => setState(() => _keys = !_keys),
        icon: const Icon(Icons.keyboard_command_key),
      ),
      IconButton(
        tooltip: w.soundOn ? ScreenStrings.soundOff : ScreenStrings.soundOn,
        isSelected: w.soundOn,
        onPressed: w.phase == WatchPhase.ended
            ? null
            : () => w.setSound(!w.soundOn),
        icon: const Icon(Icons.volume_off_outlined),
        selectedIcon: const Icon(Icons.volume_up),
      ),
    ];
  }

  Widget _mode() {
    final w = _watch;
    return SegmentedButton<bool>(
      showSelectedIcon: false,
      segments: const [
        ButtonSegment(value: false, label: Text(ScreenStrings.view)),
        ButtonSegment(value: true, label: Text(ScreenStrings.controlMode)),
      ],
      selected: {w.control},
      onSelectionChanged: w.restarting
          ? null
          : (value) => unawaited(_setControl(value.single)),
    );
  }

  /// What sits above the buttons: the transcribing line, the text field and
  /// the key pad, each only while it is wanted.
  List<Widget> _panels() {
    final w = _watch;
    final live = w.phase == WatchPhase.live;
    return [
      if (widget.dictate.phase == DictatePhase.sending)
        Padding(
          padding: const EdgeInsets.only(bottom: 4),
          child: Semantics(
            liveRegion: true,
            child: Text(ScreenStrings.transcribing(_name)),
          ),
        ),
      if (_typing)
        Padding(
          padding: const EdgeInsets.only(bottom: 6),
          child: _TypeField(
            controller: _text,
            focusNode: _textFocus,
            hint: ScreenStrings.typeHint(w.title),
            enabled: live,
            onSend: _sendText,
          ),
        ),
      if (_keys)
        Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: KeyPad(
            platform: widget.computer.platform,
            onPress: live ? _press : null,
          ),
        ),
    ];
  }

  /// Portrait: the controls under the picture.
  Widget _buildBar(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 6, 8, 6),
          child: AnimatedSize(
            duration: const Duration(milliseconds: 180),
            alignment: Alignment.bottomCenter,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                ..._panels(),
                Row(
                  children: [
                    ..._buttons(context),
                    const SizedBox(width: 8),
                    // Shrinks rather than overflows on the smallest phones.
                    Expanded(
                      child: Align(
                        alignment: AlignmentDirectional.centerEnd,
                        child: FittedBox(fit: BoxFit.scaleDown, child: _mode()),
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Landscape: the controls beside the picture, a slim column of buttons
  /// until the key pad or the text field needs the width.
  Widget _buildSide(BuildContext context) {
    final w = _watch;
    final wide = _keys || _typing;
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SafeArea(
        left: false,
        child: SizedBox(
          width: wide ? KeyPad.minWidth + 24 : 64,
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(8),
            child: wide
                ? Column(
                    children: [
                      ..._panels(),
                      Wrap(
                        alignment: WrapAlignment.center,
                        children: _buttons(context),
                      ),
                      const SizedBox(height: 8),
                      FittedBox(fit: BoxFit.scaleDown, child: _mode()),
                    ],
                  )
                : Column(
                    children: [
                      ..._buttons(context),
                      const SizedBox(height: 8),
                      IconButton(
                        tooltip: w.control
                            ? ScreenStrings.controlMode
                            : ScreenStrings.view,
                        isSelected: w.control,
                        onPressed: w.restarting
                            ? null
                            : () => unawaited(_setControl(!w.control)),
                        icon: const Icon(Icons.visibility_outlined),
                        selectedIcon: const Icon(Icons.mouse),
                      ),
                    ],
                  ),
          ),
        ),
      ),
    );
  }
}

class _TopBar extends StatelessWidget {
  const _TopBar({
    required this.title,
    required this.computerName,
    required this.status,
    required this.live,
    required this.follow,
    required this.onFollow,
    required this.onClose,
    required this.notice,
    required this.filled,
    required this.onFill,
    required this.sideways,
    required this.onRotate,
  });

  final String title;
  final String computerName;
  final String status;
  final bool live;
  final bool follow;

  /// Null while there is no text cursor to follow.
  final VoidCallback? onFollow;
  final VoidCallback onClose;
  final String? notice;

  /// The picture fills the view; [onFill] toggles it, null while there are
  /// no bars to fill.
  final bool filled;
  final VoidCallback? onFill;

  /// The viewer is on its side; [onRotate] turns it.
  final bool sideways;
  final VoidCallback onRotate;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return DecoratedBox(
      decoration: const BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Color(0xCC000000), Color(0x00000000)],
        ),
      ),
      child: SafeArea(
        bottom: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(4, 4, 8, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                children: [
                  IconButton(
                    tooltip: ScreenStrings.stopWatching,
                    onPressed: onClose,
                    icon: const Icon(Icons.close),
                  ),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.titleSmall,
                        ),
                        Row(
                          children: [
                            Icon(
                              Icons.circle,
                              size: 8,
                              color: live
                                  ? theme.colorScheme.primary
                                  : theme.colorScheme.onSurfaceVariant,
                            ),
                            const SizedBox(width: 6),
                            Semantics(
                              liveRegion: true,
                              child: Text(
                                status,
                                style: theme.textTheme.bodySmall?.copyWith(
                                  color: live
                                      ? theme.colorScheme.primary
                                      : theme.colorScheme.onSurfaceVariant,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ),
                            Flexible(
                              child: Text(
                                ' · $computerName',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: theme.textTheme.bodySmall?.copyWith(
                                  color: theme.colorScheme.onSurfaceVariant,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                  if (onFill case final onFill?)
                    IconButton(
                      tooltip: filled ? ScreenStrings.fit : ScreenStrings.fill,
                      isSelected: filled,
                      onPressed: onFill,
                      icon: const Icon(Icons.fit_screen_outlined),
                      selectedIcon: const Icon(Icons.fit_screen),
                    ),
                  IconButton(
                    tooltip: sideways
                        ? ScreenStrings.unrotate
                        : ScreenStrings.rotate,
                    onPressed: onRotate,
                    icon: Icon(
                      sideways
                          ? Icons.stay_current_portrait
                          : Icons.stay_current_landscape,
                    ),
                  ),
                  if (onFollow != null)
                    IconButton(
                      tooltip: follow
                          ? ScreenStrings.following
                          : ScreenStrings.follow,
                      isSelected: follow,
                      onPressed: onFollow,
                      icon: const Icon(Icons.center_focus_weak),
                      selectedIcon: const Icon(Icons.center_focus_strong),
                    ),
                ],
              ),
              if (notice case final notice?)
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 4, 12, 0),
                  child: Text(notice, style: theme.textTheme.bodySmall),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TypeField extends StatelessWidget {
  const _TypeField({
    required this.controller,
    required this.focusNode,
    required this.hint,
    required this.enabled,
    required this.onSend,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final String hint;
  final bool enabled;
  final VoidCallback onSend;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        Expanded(
          child: TextField(
            controller: controller,
            focusNode: focusNode,
            enabled: enabled,
            minLines: 1,
            maxLines: 3,
            inputFormatters: [typedTextLimit],
            textCapitalization: TextCapitalization.sentences,
            decoration: InputDecoration(hintText: hint, isDense: true),
          ),
        ),
        const SizedBox(width: 8),
        ValueListenableBuilder(
          valueListenable: controller,
          builder: (context, value, _) => IconButton.filled(
            tooltip: ScreenStrings.send,
            onPressed: enabled && value.text.trim().isNotEmpty ? onSend : null,
            icon: const Icon(Icons.send),
          ),
        ),
      ],
    );
  }
}
