import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../design/components.dart';
import '../../design/theme.dart';
import '../../design/tokens.dart';
import '../assistant/assistant_controller.dart';
import '../assistant/assistant_widgets.dart';
import '../assistant/strings.dart';
import '../dictate/dictate_screen.dart' show typedTextLimit;
import '../dictate/key_pad.dart';
import '../dictate/strings.dart';
import 'live_controller.dart';
import 'live_dock.dart' show watchStatus;
import 'live_protocol.dart';
import 'strings.dart';
import 'view_geometry.dart';
import 'watch_controller.dart';

/// A screen as the picker and the viewer name it.
String screenLabel(RemoteScreen screen) =>
    screen.name.isEmpty ? ScreenStrings.untitledScreen : screen.name;

/// The live view, full screen (design spec §4.8): the same watch as the
/// small view, bigger. Under the picture (beside it on its side) are the
/// latest answer, the keys and typing on demand, the assistant's composer,
/// View or Control, and the toolbar. Back returns to the small view; Stop
/// ends the watch.
///
/// The gestures on the picture:
/// - View (the default): pinch zooms, one finger drags the zoomed picture
///   around. Nothing on the computer is clicked or scrolled.
/// - Control: one tap is a left click, a double tap a double click, a long
///   press a right click, and two fingers dragged together (without
///   pinching) scroll the computer, one wheel notch per 40 points, on the
///   axis they first moved along. Pinch still zooms and one finger still
///   drags the zoomed picture, since the protocol has no mouse drag.
/// - In both, keys (the pad) and typed text go to the watched window.
///
/// A tap on the bars beside the picture clicks nothing. A click on a control
/// that sends, buys, posts or deletes is asked about first ([_confirm]).
class WatchScreen extends StatefulWidget {
  const WatchScreen({
    super.key,
    required this.watch,
    required this.live,
    this.assistant,
  });

  final WatchController watch;
  final LiveController live;

  /// The app's assistant chat; null leaves the assistant bar out.
  final AssistantController? assistant;

  @override
  State<WatchScreen> createState() => _WatchScreenState();
}

class _WatchScreenState extends State<WatchScreen>
    with SingleTickerProviderStateMixin {
  WatchController get _watch => widget.watch;
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

  /// The typing field is open.
  var _typing = false;
  final _text = TextEditingController();
  final _textFocus = FocusNode();

  // One gesture on the picture in Control: where it started, whether its
  // two fingers scroll (null: not decided yet), on which axis, and the
  // notches already sent.
  Offset _gestureFocal = Offset.zero;
  bool? _gestureScrolls;
  Axis _scrollAxis = Axis.vertical;
  var _scrolled = 0;
  Offset? _scrollAt;
  Offset? _doubleTapAt;

  /// Two fingers are scrolling the computer: the viewer does not pan (or
  /// fling) the picture meanwhile.
  var _scrolling = false;

  /// The latest answer's card is open, and the turn it shows. Only turns
  /// asked after the viewer opened are shown over the picture.
  var _answerOpen = true;
  AssistantTurn? _answerTurn;
  AssistantTurn? _turnAtOpen;
  StreamSubscription<String>? _keyErrors;

  @override
  void initState() {
    super.initState();
    _watch.addListener(_onWatch);
    _move.addListener(() {
      if (_moveTo case final to?) _transform.value = to.value;
    });
    _keyErrors = _watch.keyErrors.listen(_onKeyError);
    widget.assistant?.addListener(_onAssistant);
    _turnAtOpen = _answerTurn = widget.assistant?.turns.lastOrNull;
  }

  void _onKeyError(String code) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(ScreenStrings.error(code, _name))));
  }

  void _pressKeys(List<String> keys) {
    Haptic.tick();
    _watch.pressKeys(keys);
  }

  void _snack(String text) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(text)));
  }

  /// Says why input was not delivered. Nothing for the watch ending, for a
  /// click the user chose not to confirm, or for one still waiting (`busy`).
  void _showInputError(InputResult result) {
    final error = result.error;
    if (error == null ||
        const {'cancelled', 'confirm', 'busy'}.contains(error)) {
      return;
    }
    _snack(ScreenStrings.error(error, _name));
  }

  /// Taps and scrolls reach the computer now.
  bool get _controlLive =>
      _watch.control && _watch.phase == WatchPhase.live && _watch.channelOpen;

  void _setControl(bool control) {
    _snack(control ? ScreenStrings.controlHint : ScreenStrings.viewHint);
    _watch.setControl(control);
  }

  /// A click at [point] (viewport coordinates), mapped through the zoom,
  /// the pan and the letterbox to the computer's picture.
  Future<void> _click(Offset point, {bool right = false, int count = 1}) async {
    final at = pictureCoords(
      point: point,
      transform: _transform.value,
      viewport: _viewport,
      picture: _picture,
    );
    if (at == null) return;
    Haptic.tap();
    _showInputError(
      await _watch.click(at, right: right, count: count, confirm: _confirm),
    );
  }

  /// Asks before a click on a control the computer says sends, buys, posts
  /// or deletes; true for Do it.
  Future<bool> _confirm(String? label) async {
    if (!mounted) return false;
    Haptic.tick();
    return confirmDialog(
      context,
      title: label == null || label.isEmpty
          ? ScreenStrings.confirmUnlabelled
          : ScreenStrings.confirmTitle(label),
      body: ScreenStrings.confirmBody,
      action: ScreenStrings.doIt,
      cancel: ScreenStrings.cancel,
      destructive: true,
    );
  }

  void _toggleTyping() {
    Haptic.tick();
    setState(() => _typing = !_typing);
    if (_typing) {
      _textFocus.requestFocus();
    } else {
      _textFocus.unfocus();
    }
  }

  /// Sends the typed text; it stays in the field when it was not delivered.
  Future<void> _sendText() async {
    final text = _text.text;
    if (text.trim().isEmpty) return;
    Haptic.tap();
    final result = await _watch.typeText(text);
    if (!mounted) return;
    if (result.ok) {
      _text.clear();
    } else {
      _showInputError(result);
    }
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
          ?.where((d) => d.id == _watch.computer.id)
          .firstOrNull ??
      _watch.computer;

  /// The assistant bar is offered.
  bool get _canAsk => widget.assistant != null && hasAssistant(_computer);

  @override
  void dispose() {
    unawaited(_keyErrors?.cancel());
    widget.assistant?.removeListener(_onAssistant);
    _text.dispose();
    _textFocus.dispose();
    if (_held != null) _hold(null);
    _watch.removeListener(_onWatch);
    _move.dispose();
    _transform.dispose();
    super.dispose();
  }

  String get _name => _watch.computer.name;

  /// Holds the viewer sideways or upright; null lets the phone decide again.
  /// Best effort: without the platform (tests) it simply does not.
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

  /// In Control, two fingers moving together without pinching scroll the
  /// computer instead of moving the view: decided once per gesture, after
  /// 24 points of movement, and on the axis the fingers moved along most.
  void _onInteractionUpdate(ScaleUpdateDetails details) {
    final from = _gestureFrom;
    if (!_controlLive || from == null || details.pointerCount < 2) return;
    final moved = details.localFocalPoint - _gestureFocal;
    if (_gestureScrolls == null) {
      if ((details.scale - 1).abs() > 0.08) {
        _gestureScrolls = false;
      } else if (moved.distance > 24) {
        _gestureScrolls = true;
        _scrollAxis = moved.dy.abs() >= moved.dx.abs()
            ? Axis.vertical
            : Axis.horizontal;
      }
    }
    if (_gestureScrolls != true) return;
    if (!_scrolling) setState(() => _scrolling = true);
    _transform.value = from;
    final vertical = _scrollAxis == Axis.vertical;
    final notches = scrollNotches(vertical ? moved.dy : moved.dx) - _scrolled;
    final at = _scrollAt;
    if (notches == 0 || at == null) return;
    _scrolled += notches;
    unawaited(
      _watch
          .scroll(at, dy: vertical ? notches : 0, dx: vertical ? 0 : notches)
          .then(_showInputError),
    );
  }

  void _onInteractionEnd(ScaleEndDetails details) {
    if (_scrolling) setState(() => _scrolling = false);
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

  void _back() => unawaited(Navigator.maybePop(context));

  void _stop() {
    Haptic.firm();
    widget.live.close();
    _back();
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
            backgroundColor: TalvioColors.mediaBg,
            body: MediaQuery.orientationOf(context) == Orientation.landscape
                ? Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(child: _buildStage(context)),
                      SizedBox(
                        width: 360,
                        child: _glass(
                          context,
                          SafeArea(
                            left: false,
                            child: Column(
                              children: [
                                if (_answer(context) case final answer?)
                                  Expanded(child: answer)
                                else
                                  const Spacer(),
                                ..._controls(context),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ],
                  )
                : Column(
                    children: [
                      Expanded(child: _buildStage(context)),
                      _glass(
                        context,
                        SafeArea(
                          top: false,
                          child: LayoutBuilder(
                            builder: (context, constraints) => Column(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                if (_answer(context) case final answer?)
                                  ConstrainedBox(
                                    constraints: BoxConstraints(
                                      maxHeight:
                                          MediaQuery.sizeOf(context).height *
                                          0.4,
                                    ),
                                    child: answer,
                                  ),
                                ..._controls(context),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ],
                  ),
          ),
        ),
      ),
    );
  }

  /// The bottom (or side) area: graphite at 92% over a blur.
  Widget _glass(BuildContext context, Widget child) => ClipRect(
    child: BackdropFilter(
      filter: ui.ImageFilter.blur(sigmaX: 20, sigmaY: 20),
      child: ColoredBox(
        color: context.talvio.canvas.withValues(alpha: 0.92),
        child: child,
      ),
    ),
  );

  /// The keys and typing when open, the composer, View or Control, and the
  /// toolbar.
  List<Widget> _controls(BuildContext context) => [
    if (_typing)
      Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 4, 0),
        child: _TypeField(
          controller: _text,
          focusNode: _textFocus,
          hint: ScreenStrings.typeHint(_watch.title),
          enabled: _watch.channelOpen,
          onSend: () => unawaited(_sendText()),
        ),
      ),
    if (_keysOpen)
      Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 0),
        child: KeyPad(
          platform: _computer.platform,
          onPress: _watch.channelOpen ? _pressKeys : null,
        ),
      ),
    if (_canAsk)
      AssistantInput(
        controller: widget.assistant!,
        enabled: _assistantReady,
        dense: true,
        margin: const EdgeInsets.fromLTRB(12, 8, 12, 4),
      ),
    Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 280),
          child: SegmentedPill<bool>(
            height: 32,
            options: const [
              (false, ScreenStrings.view, AppIcons.view),
              (true, ScreenStrings.controlMode, AppIcons.control),
            ],
            selected: _watch.control,
            onChanged: _watch.phase == WatchPhase.ended ? null : _setControl,
          ),
        ),
      ),
    ),
    _toolbar(context),
  ];

  Widget _buildStage(BuildContext context) {
    final w = _watch;
    final link = w.link;
    final notice = switch (w.watched?.state) {
      'minimized' => ScreenStrings.minimized,
      'covered' => ScreenStrings.covered,
      _ => null,
    };
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
              final control = _controlLive;
              // Without callbacks (View) this adds no recognizers, so the
              // viewer's pinch and pan are all there is.
              return GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTapUp: control
                    ? (d) => unawaited(_click(d.localPosition))
                    : null,
                onDoubleTapDown: control
                    ? (d) => _doubleTapAt = d.localPosition
                    : null,
                onDoubleTap: control
                    ? () {
                        if (_doubleTapAt case final at?) {
                          unawaited(_click(at, count: 2));
                        }
                      }
                    : null,
                onLongPressStart: control
                    ? (d) => unawaited(_click(d.localPosition, right: true))
                    : null,
                child: InteractiveViewer(
                  transformationController: _transform,
                  minScale: 1,
                  maxScale: 8,
                  panEnabled: !_scrolling,
                  onInteractionStart: _onInteractionStart,
                  onInteractionUpdate: _onInteractionUpdate,
                  onInteractionEnd: _onInteractionEnd,
                  child: SizedBox.fromSize(
                    size: viewport,
                    child: link == null || !w.foreground
                        ? const ColoredBox(color: TalvioColors.mediaBg)
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
            status: watchStatus(w),
            live: w.phase == WatchPhase.live && !w.reconnecting,
            onBack: _back,
            onMore: () => _openOptions(context),
            notice: notice,
          ),
        ),
        if (w.phase == WatchPhase.ended)
          Center(child: _endedCard(context))
        else if (w.phase != WatchPhase.live)
          Center(child: _waiting(context)),
      ],
    );
  }

  /// Only a picture that leaves bars can fill the view.
  bool get _canFill =>
      _filled ||
      (_watch.link != null &&
          !_viewport.isEmpty &&
          fillScale(_picture, _viewport) >= 1.05);

  /// Fit or Fill, sideways or upright, and Follow, in a sheet.
  Future<void> _openOptions(BuildContext context) {
    Haptic.tick();
    final orientation = MediaQuery.orientationOf(context);
    final sideways = orientation == Orientation.landscape;
    return showTalvioSheet<void>(
      context,
      title: ScreenStrings.moreOptions,
      builder: (sheet) {
        final t = sheet.talvio;
        Widget? check(bool on) =>
            on ? Icon(AppIcons.check, size: 20, color: t.accent) : null;
        return SingleChildScrollView(
          child: SheetRows(
            children: [
              ListRow(
                leading: RowGlyph(_filled ? AppIcons.fit : AppIcons.fill),
                title: _filled ? ScreenStrings.fit : ScreenStrings.fill,
                semanticLabel: _filled ? ScreenStrings.fit : ScreenStrings.fill,
                enabled: _canFill,
                onTap: () {
                  Navigator.pop(sheet);
                  _toggleFill();
                },
              ),
              ListRow(
                leading: const RowGlyph(AppIcons.rotate),
                title: sideways ? ScreenStrings.unrotate : ScreenStrings.rotate,
                semanticLabel: sideways
                    ? ScreenStrings.unrotate
                    : ScreenStrings.rotate,
                onTap: () {
                  Navigator.pop(sheet);
                  _rotate(orientation);
                },
              ),
              ListRow(
                leading: RowGlyph(
                  _follow ? AppIcons.followOn : AppIcons.follow,
                  color: _follow ? t.ink : null,
                ),
                title: ScreenStrings.follow,
                semanticLabel: _follow
                    ? ScreenStrings.following
                    : ScreenStrings.follow,
                selected: _follow,
                enabled: _watch.caret != null,
                trailing: check(_follow),
                onTap: () {
                  Navigator.pop(sheet);
                  _setFollow(!_follow);
                },
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _waiting(BuildContext context) {
    final approval = _watch.phase == WatchPhase.approval;
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.all(32),
      child: Semantics(
        liveRegion: true,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox.square(
              dimension: 20,
              child: approval
                  ? Icon(AppIcons.waiting, size: 20, color: t.ink)
                  : Spinner(color: t.ink),
            ),
            const SizedBox(height: Space.s8),
            Text(
              approval ? DictateStrings.approval(_name) : watchStatus(_watch),
              textAlign: TextAlign.center,
              style: TalvioType.rowTitle.copyWith(color: t.ink),
            ),
            if (approval) ...[
              const SizedBox(height: Space.s4),
              Text(
                ScreenStrings.approvalDetail,
                textAlign: TextAlign.center,
                style: TalvioType.footnote.copyWith(color: t.muted),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _endedCard(BuildContext context) {
    final error = _watch.endError;
    final t = context.talvio;
    return Padding(
      padding: const EdgeInsets.all(Space.s24),
      child: Container(
        padding: const EdgeInsets.all(Space.s16),
        constraints: const BoxConstraints(maxWidth: 360),
        decoration: BoxDecoration(
          color: t.card,
          borderRadius: BorderRadius.circular(Radii.lg),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Semantics(
              liveRegion: true,
              child: Text(
                ScreenStrings.ended,
                style: TalvioType.rowTitle.copyWith(color: t.ink),
              ),
            ),
            const SizedBox(height: Space.s4),
            Text(
              error == null
                  ? ScreenStrings.stoppedByYou
                  : ScreenStrings.error(error, _name),
              textAlign: TextAlign.center,
              style: TalvioType.footnote.copyWith(color: t.muted),
            ),
            const SizedBox(height: Space.s12),
            Wrap(
              alignment: WrapAlignment.center,
              spacing: Space.s8,
              children: [
                Btn(
                  ScreenStrings.close,
                  kind: BtnKind.ghostNeutral,
                  compact: true,
                  onPressed: _stop,
                ),
                if (error != 'target_gone')
                  Btn(
                    ScreenStrings.watchAgain,
                    kind: BtnKind.secondary,
                    compact: true,
                    onPressed: _watch.start,
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  /// Sound, Keys, the answer, and Stop.
  Widget _toolbar(BuildContext context) {
    final t = context.talvio;
    final w = _watch;
    final ended = w.phase == WatchPhase.ended;
    final hasAnswer = _canAsk && _shownTurn != null;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: SizedBox(
        height: 52,
        child: Row(
          children: [
            IconBtn(
              tooltip: w.soundOn
                  ? ScreenStrings.soundOff
                  : ScreenStrings.soundOn,
              selected: w.soundOn,
              icon: w.soundOn ? AppIcons.soundOn : AppIcons.soundOff,
              onPressed: ended ? null : () => w.setSound(!w.soundOn),
            ),
            IconBtn(
              tooltip: _typing ? ScreenStrings.hideType : ScreenStrings.type,
              selected: _typing,
              icon: _typing ? AppIcons.typeTextOn : AppIcons.typeText,
              onPressed: ended ? null : _toggleTyping,
            ),
            IconBtn(
              tooltip: _keysOpen
                  ? AssistantStrings.hideKeys
                  : AssistantStrings.keys,
              selected: _keysOpen,
              icon: _keysOpen ? AppIcons.keyboardFill : AppIcons.keyboard,
              onPressed: ended
                  ? null
                  : () {
                      Haptic.tick();
                      setState(() => _keysOpen = !_keysOpen);
                    },
            ),
            if (_canAsk)
              IconBtn(
                tooltip: _answerOpen && hasAnswer
                    ? AssistantStrings.hideAnswer
                    : AssistantStrings.showAnswer,
                selected: _answerOpen && hasAnswer,
                icon: _answerOpen && hasAnswer
                    ? AppIcons.answerOn
                    : AppIcons.answer,
                onPressed: hasAnswer
                    ? () {
                        Haptic.tick();
                        setState(() => _answerOpen = !_answerOpen);
                      }
                    : null,
              ),
            const Spacer(),
            Padding(
              padding: const EdgeInsetsDirectional.only(end: 8),
              child: Btn(
                ScreenStrings.stop,
                kind: BtnKind.secondary,
                compact: true,
                icon: AppIcons.stop,
                foreground: t.danger,
                borderColor: t.danger,
                onPressed: _stop,
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// The assistant can be asked now.
  bool get _assistantReady =>
      _computer.online &&
      AppServices.of(context).relay.status.value == RelayStatus.connected;

  /// The latest answer's card, while there is one and it is open.
  Widget? _answer(BuildContext context) {
    final turn = _shownTurn;
    if (!_canAsk || turn == null || !_answerOpen) return null;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 8),
      child: _AnswerCard(
        turn: turn,
        controller: widget.assistant!,
        onHide: () => setState(() => _answerOpen = false),
      ),
    );
  }
}

/// The latest answer in live view: the question, the answer as it streams
/// in, what was done and anything waiting for Do it.
class _AnswerCard extends StatelessWidget {
  const _AnswerCard({
    required this.turn,
    required this.controller,
    required this.onHide,
  });

  final AssistantTurn turn;
  final AssistantController controller;
  final VoidCallback onHide;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Container(
      decoration: BoxDecoration(
        color: t.card,
        borderRadius: BorderRadius.circular(Radii.lg),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsetsDirectional.only(start: 16, end: 4),
            child: Row(
              children: [
                Icon(AppIcons.sparkleFill, size: 16, color: t.accent),
                const SizedBox(width: Space.s8),
                Expanded(
                  child: Semantics(
                    label: AssistantStrings.assistantOn(turn.computerName),
                    child: Text(
                      turn.question ?? AssistantStrings.voiceQuestion,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TalvioType.footnote.copyWith(color: t.muted),
                    ),
                  ),
                ),
                IconBtn(
                  tooltip: AssistantStrings.hideAnswer,
                  onPressed: onHide,
                  color: t.muted,
                  icon: AppIcons.caretDown,
                ),
              ],
            ),
          ),
          Flexible(
            child: SingleChildScrollView(
              reverse: true,
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
              child: TurnView(
                turn: turn,
                controller: controller,
                compact: true,
                showQuestion: false,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The field that types into the watched window, with its send button.
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
        ValueListenableBuilder(
          valueListenable: controller,
          builder: (context, value, _) => IconBtn(
            tooltip: ScreenStrings.sendText,
            icon: AppIcons.send,
            onPressed: enabled && value.text.trim().isNotEmpty ? onSend : null,
          ),
        ),
      ],
    );
  }
}

class _TopBar extends StatelessWidget {
  const _TopBar({
    required this.title,
    required this.computerName,
    required this.status,
    required this.live,
    required this.onBack,
    required this.onMore,
    required this.notice,
  });

  final String title;
  final String computerName;
  final String status;
  final bool live;
  final VoidCallback onBack;
  final VoidCallback onMore;
  final String? notice;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        DecoratedBox(
          decoration: const BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [Color(0x99000000), Color(0x00000000)],
            ),
          ),
          child: SafeArea(
            bottom: false,
            child: SizedBox(
              height: 72,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 4),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    IconBtn(
                      tooltip: ScreenStrings.exitFullScreen,
                      onPressed: onBack,
                      color: Colors.white,
                      icon: AppIcons.exitFullScreen,
                    ),
                    const SizedBox(width: Space.s4),
                    Expanded(
                      child: Padding(
                        padding: const EdgeInsets.only(top: 3),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Text(
                              title,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TalvioType.rowTitle.copyWith(
                                color: Colors.white,
                              ),
                            ),
                            const SizedBox(height: Space.s2),
                            Row(
                              children: [
                                StatusDot(
                                  color: live ? t.live : t.faint,
                                  blink: live,
                                ),
                                const SizedBox(width: 6),
                                Flexible(
                                  child: Semantics(
                                    liveRegion: true,
                                    child: Text(
                                      '$status · $computerName',
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: TalvioType.footnote.copyWith(
                                        color: Colors.white.withValues(
                                          alpha: 0.7,
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ),
                    IconBtn(
                      tooltip: ScreenStrings.moreOptions,
                      onPressed: onMore,
                      color: Colors.white,
                      icon: AppIcons.more,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
        if (notice case final notice?)
          InlineBanner(
            message: notice,
            margin: const EdgeInsets.fromLTRB(16, 0, 16, 0),
          ),
      ],
    );
  }
}
