import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../shell/shell_scope.dart';
import '../../theme.dart';
import '../assistant/assistant_controller.dart';
import '../assistant/assistant_scope.dart';
import '../assistant/assistant_widgets.dart';
import '../assistant/strings.dart';
import '../dictate/dictate_controller.dart';
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

/// The route of live view: a window or screen of [computer], to watch.
/// With [assistant], the viewer has the assistant bar, in the same chat as
/// the Assistant page, which [onOpenAssistant] shows.
Route<void> watchRoute({
  required Device computer,
  required WatchTarget target,
  RtcLinkFactory? linkFactory,
  AssistantController? assistant,
  VoidCallback? onOpenAssistant,
}) => MaterialPageRoute(
  builder: (_) => WatchScreen(
    computer: computer,
    target: target,
    linkFactory: linkFactory ?? WebRtcLink.create,
    assistant: assistant,
    onOpenAssistant: onOpenAssistant,
  ),
);

/// Opens live view of [target] on [dictate]'s computer. The watched window
/// is also where the Send page's dictation and typing go next: [window], or
/// for a screen wherever the keyboard is. Completes when the viewer closes.
Future<void> openWatch(
  BuildContext context,
  DictateController dictate,
  WatchTarget target, {
  RemoteWindow? window,
}) async {
  final computer = dictate.computer;
  if (computer == null) return;
  dictate.chooseWindow(window);
  final select = ShellScope.maybeOf(context)?.select;
  await Navigator.of(context).push(
    watchRoute(
      computer: computer,
      target: target,
      assistant: AssistantScope.maybeOf(context),
      onOpenAssistant: select == null ? null : () => select('assistant'),
    ),
  );
}

/// Live view: pinch and drag move around the picture, and nothing on the
/// computer is clicked or scrolled from here. Under the picture (beside it
/// on its side) are the keys, pressed in the watched window, and the
/// assistant bar.
class WatchScreen extends StatefulWidget {
  const WatchScreen({
    super.key,
    required this.computer,
    required this.target,
    required this.linkFactory,
    this.assistant,
    this.onOpenAssistant,
  });

  final Device computer;
  final WatchTarget target;
  final RtcLinkFactory linkFactory;

  /// The app's assistant chat; null leaves the assistant bar out.
  final AssistantController? assistant;

  /// Shows the Assistant page (after the viewer closes).
  final VoidCallback? onOpenAssistant;

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

  /// Keep the text cursor in view. Any pan or zoom by hand turns it off.
  var _follow = true;

  /// The picture fills the view, cropped, rather than fits in it whole.
  var _filled = false;

  /// The orientation the viewer is held in; null to follow the phone.
  Orientation? _held;
  Size _viewport = Size.zero;
  Caret? _followed;

  /// Where the view was when the pan or zoom in progress began.
  Matrix4? _gestureFrom;

  /// The key pad is open.
  var _keysOpen = false;

  /// On its side: the assistant's panel is open. Upright the bar is always
  /// there.
  var _assistantOpen = false;

  /// The latest answer's card is open, and the turn it shows. Only turns
  /// asked after the viewer opened are shown over the picture.
  var _answerOpen = true;
  AssistantTurn? _answerTurn;
  AssistantTurn? _turnAtOpen;
  StreamSubscription<String>? _keyErrors;

  @override
  void initState() {
    super.initState();
    _watch = WatchController(
      relay: AppServices.of(context).relay,
      computer: widget.computer,
      target: widget.target,
      linkFactory: widget.linkFactory,
    )..addListener(_onWatch);
    _move.addListener(() {
      if (_moveTo case final to?) _transform.value = to.value;
    });
    _lifecycle = AppLifecycleListener(
      onResume: () => _watch.setForeground(true),
      onPause: () => _watch.setForeground(false),
    );
    _keepAwake(true);
    _keyErrors = _watch.keyErrors.listen(_onKeyError);
    widget.assistant?.addListener(_onAssistant);
    _turnAtOpen = _answerTurn = widget.assistant?.turns.lastOrNull;
    _watch.start();
  }

  void _onKeyError(String code) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(ScreenStrings.error(code, _name))));
  }

  void _pressKeys(List<String> keys) {
    unawaited(HapticFeedback.selectionClick());
    _watch.pressKeys(keys);
  }

  /// A new question opens its card again.
  void _onAssistant() {
    final last = widget.assistant?.turns.lastOrNull;
    if (!identical(last, _answerTurn)) {
      _answerTurn = last;
      if (!_answerOpen) setState(() => _answerOpen = true);
    }
  }

  /// The turn to show over the picture: the latest, if asked here.
  AssistantTurn? get _shownTurn {
    final last = widget.assistant?.turns.lastOrNull;
    return last == null || identical(last, _turnAtOpen) ? null : last;
  }

  /// The watched computer, as the relay knows it now.
  Device get _computer =>
      AppServices.of(context).relay.devices.value
          ?.where((d) => d.id == widget.computer.id)
          .firstOrNull ??
      widget.computer;

  /// The assistant bar is offered.
  bool get _canAsk => widget.assistant != null && hasAssistant(_computer);

  /// Closes the viewer and shows the Assistant page.
  void _openAssistant() {
    final open = widget.onOpenAssistant;
    unawaited(Navigator.maybePop(context));
    open?.call();
  }

  @override
  void dispose() {
    _keepAwake(false);
    unawaited(_keyErrors?.cancel());
    widget.assistant?.removeListener(_onAssistant);
    if (_held != null) _hold(null);
    _lifecycle.dispose();
    _watch
      ..removeListener(_onWatch)
      ..dispose();
    _move.dispose();
    _transform.dispose();
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

  void _onInteractionStart(ScaleStartDetails details) {
    _move.stop();
    _gestureFrom = _transform.value.clone();
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
          listenable: Listenable.merge([
            _watch,
            ?widget.assistant,
            AppServices.of(context).relay.devices,
          ]),
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
                      Expanded(
                        child: LayoutBuilder(
                          builder: (context, constraints) => Stack(
                            children: [
                              Positioned.fill(child: _buildStage(context)),
                              ?_answerOverlay(context, constraints),
                            ],
                          ),
                        ),
                      ),
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
              return InteractiveViewer(
                transformationController: _transform,
                minScale: 1,
                maxScale: 8,
                onInteractionStart: _onInteractionStart,
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
        if (w.phase == WatchPhase.ended)
          Center(child: _endedCard(context))
        else if (w.phase != WatchPhase.live)
          Center(child: _waiting(context)),
      ],
    );
  }

  String _statusText() {
    final w = _watch;
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

  /// Sound on or off.
  Widget _sound() {
    final w = _watch;
    return IconButton(
      tooltip: w.soundOn ? ScreenStrings.soundOff : ScreenStrings.soundOn,
      isSelected: w.soundOn,
      onPressed: w.phase == WatchPhase.ended
          ? null
          : () => w.setSound(!w.soundOn),
      icon: const Icon(Icons.volume_off_outlined),
      selectedIcon: const Icon(Icons.volume_up),
    );
  }

  void _close() => unawaited(Navigator.maybePop(context));

  /// Keys open or closed.
  Widget _keysToggle() => IconButton(
    tooltip: _keysOpen ? AssistantStrings.hideKeys : AssistantStrings.keys,
    isSelected: _keysOpen,
    onPressed: _watch.phase == WatchPhase.ended
        ? null
        : () => setState(() => _keysOpen = !_keysOpen),
    icon: const Icon(Icons.keyboard_outlined),
    selectedIcon: const Icon(Icons.keyboard),
  );

  Widget _keyPad() => Padding(
    padding: const EdgeInsets.only(bottom: 8),
    child: KeyPad(
      platform: _computer.platform,
      onPress: _watch.channelOpen ? _pressKeys : null,
    ),
  );

  /// The assistant can be asked now.
  bool get _assistantReady =>
      _computer.online &&
      AppServices.of(context).relay.status.value == RelayStatus.connected;

  Widget _assistantInput() => AssistantInput(
    controller: widget.assistant!,
    enabled: _assistantReady,
    dense: true,
  );

  /// Upright: the latest answer over the bottom of the picture, at most
  /// about a third of it; a small button once it is hidden.
  Widget? _answerOverlay(BuildContext context, BoxConstraints constraints) {
    final turn = _shownTurn;
    if (!_canAsk || turn == null) return null;
    if (!_answerOpen) {
      return Positioned(
        right: 8,
        bottom: 8,
        child: FilledButton.tonalIcon(
          onPressed: () => setState(() => _answerOpen = true),
          icon: const Icon(Icons.chat_bubble_outline, size: 18),
          label: const Text(AssistantStrings.showAnswer),
        ),
      );
    }
    return Positioned(
      left: 8,
      right: 8,
      bottom: 8,
      child: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: constraints.maxHeight * 0.35),
        child: _AnswerCard(
          turn: turn,
          controller: widget.assistant!,
          onHide: () => setState(() => _answerOpen = false),
          onOpen: widget.onOpenAssistant == null ? null : _openAssistant,
        ),
      ),
    );
  }

  /// Upright: the keys when open, the assistant bar, then sound, Keys and
  /// Stop.
  Widget _buildBar(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 4, 8, 4),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (_keysOpen) _keyPad(),
              if (_canAsk) _assistantInput(),
              Row(
                children: [
                  _sound(),
                  _keysToggle(),
                  const Spacer(),
                  FilledButton.tonalIcon(
                    onPressed: _close,
                    icon: const Icon(Icons.stop_rounded),
                    label: const Text(ScreenStrings.stop),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// On its side: a slim column of buttons beside the picture, and beside
  /// it, while open, a panel with the latest answer, the keys and the
  /// assistant bar.
  Widget _buildSide(BuildContext context) {
    final asking = _canAsk && _assistantOpen;
    final turn = asking ? _shownTurn : null;
    final width = MediaQuery.sizeOf(context).width;
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SafeArea(
        left: false,
        child: Row(
          children: [
            if (asking || _keysOpen)
              SizedBox(
                width: (width * 0.45).clamp(KeyPad.minWidth + 16, 400),
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(8, 8, 0, 8),
                  child: Column(
                    children: [
                      if (turn != null)
                        Expanded(
                          child: _AnswerCard(
                            turn: turn,
                            controller: widget.assistant!,
                            onHide: null,
                            onOpen: widget.onOpenAssistant == null
                                ? null
                                : _openAssistant,
                          ),
                        )
                      else
                        const Spacer(),
                      const SizedBox(height: 8),
                      if (_keysOpen) _keyPad(),
                      if (asking) _assistantInput(),
                    ],
                  ),
                ),
              ),
            SizedBox(
              width: 64,
              child: SingleChildScrollView(
                padding: const EdgeInsets.all(8),
                child: Column(
                  children: [
                    _sound(),
                    const SizedBox(height: 8),
                    _keysToggle(),
                    if (_canAsk) ...[
                      const SizedBox(height: 8),
                      IconButton(
                        tooltip: _assistantOpen
                            ? AssistantStrings.hideAssistant
                            : AssistantStrings.askHere,
                        isSelected: _assistantOpen,
                        onPressed: () =>
                            setState(() => _assistantOpen = !_assistantOpen),
                        icon: const Icon(Icons.chat_bubble_outline),
                        selectedIcon: const Icon(Icons.chat_bubble),
                      ),
                    ],
                    const SizedBox(height: 8),
                    IconButton.filledTonal(
                      tooltip: ScreenStrings.stop,
                      onPressed: _close,
                      icon: const Icon(Icons.stop_rounded),
                    ),
                  ],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// The latest answer in live view: the question, the answer as it streams
/// in, what was done and anything waiting for Do it. Tapping it opens the
/// Assistant page.
class _AnswerCard extends StatelessWidget {
  const _AnswerCard({
    required this.turn,
    required this.controller,
    required this.onHide,
    required this.onOpen,
  });

  final AssistantTurn turn;
  final AssistantController controller;
  final VoidCallback? onHide;
  final VoidCallback? onOpen;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      margin: EdgeInsets.zero,
      clipBehavior: Clip.antiAlias,
      color: theme.colorScheme.surfaceContainerHigh.withValues(alpha: 0.96),
      child: InkWell(
        onTap: onOpen,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                const SizedBox(width: 12),
                Icon(
                  Icons.chat_bubble_outline,
                  size: 16,
                  color: theme.colorScheme.primary,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    AssistantStrings.assistantOn(turn.computerName),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.labelLarge,
                  ),
                ),
                if (onOpen != null)
                  IconButton(
                    tooltip: AssistantStrings.openAssistant,
                    visualDensity: VisualDensity.compact,
                    onPressed: onOpen,
                    icon: const Icon(Icons.open_in_full, size: 18),
                  ),
                if (onHide != null)
                  IconButton(
                    tooltip: AssistantStrings.hideAnswer,
                    visualDensity: VisualDensity.compact,
                    onPressed: onHide,
                    icon: const Icon(Icons.expand_more),
                  ),
              ],
            ),
            Flexible(
              child: SingleChildScrollView(
                reverse: true,
                padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                child: TurnView(
                  turn: turn,
                  controller: controller,
                  compact: true,
                ),
              ),
            ),
          ],
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
