import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../core/relay_protocol.dart';
import '../../core/session_store.dart';
import '../screen/live_protocol.dart';
import 'recent_dictations.dart';
import 'remote_windows.dart';

/// The request kinds (and computer capabilities) of this feature.
abstract final class DictateKind {
  static const dictate = 'dictate';
  static const text = 'text';
}

enum Deliver { paste, keep }

enum DictatePhase { idle, recording, sending }

/// How the last request ended. [error] is a result code from the computer or
/// one of the phone's own: offline, not_connected, interrupted, timeout,
/// mic_denied, mic_failed.
class DictateOutcome {
  const DictateOutcome.done({
    required String this.computer,
    required this.text,
    required this.pasted,
    required this.typed,
    this.window,
  }) : error = null;

  const DictateOutcome.failed(
    String this.error, {
    this.computer,
    this.typed = false,
  }) : text = null,
       pasted = false,
       window = null;

  final String? computer;
  final String? text;
  final bool pasted;
  final String? error;

  /// Typed text rather than a recording.
  final bool typed;

  /// The window the text went to; null for wherever the cursor was.
  final RemoteWindow? window;
}

/// The protocol allows 5 minutes of audio; stopping a second early keeps the
/// recorder's start and stop latency from pushing a full recording over.
const maxRecording = Duration(minutes: 4, seconds: 59);

/// The wait for a result beyond the recording's own length (PROTOCOL.md).
const _resultTimeout = Duration(seconds: 60);

/// The wait while the computer asks its user to allow this phone; its own
/// prompt gives up after 2 minutes (PROTOCOL.md, Approval).
const _approvalTimeout = Duration(minutes: 3);

/// The wait for a `windows.list` or `windows.keys` result: nothing to
/// transcribe, so it is quick unless the computer asks to allow this phone.
const _windowsTimeout = Duration(seconds: 20);

/// Key presses waiting for their result at most; taps beyond it are ignored
/// rather than queued, so a stuck computer is not sent a burst of keys later.
const maxPendingKeys = 3;

/// How long the "Press Enter" chip stays after a paste.
const enterOfferFor = Duration(seconds: 8);

/// Errors a resend of the same recording cannot fix.
const _final = {'bad_audio', 'no_speech', 'unsupported'};

/// The account's computers, online first (each group keeps the server's
/// order, which is by name).
List<Device> computersOf(List<Device> devices) => [
  ...devices.where((d) => d.isComputer && d.online),
  ...devices.where((d) => d.isComputer && !d.online),
];

/// The computer to send to: the remembered one while it is on the account
/// (online or not); with nothing remembered, the account's only computer.
/// Null means the user has to pick, so a computer that joins the account
/// later is never chosen for them, not even once the remembered one has
/// left (PROTOCOL.md, Approval).
Device? pickComputer(List<Device> computers, String? chosenId) =>
    chosenId == null
    ? (computers.length == 1 ? computers.single : null)
    : computers.where((d) => d.id == chosenId).firstOrNull;

class DictateController extends ChangeNotifier {
  DictateController({required this._relay, required this._prefs})
    : recents = RecentDictations(_prefs) {
    unawaited(_sweepRecordings());
  }

  static const _chosenKey = '${accountPrefsPrefix}dictate.computer';
  static const _deliverKey = 'dictate.deliver';
  static const _filePrefix = 'dictation-';

  final RelayClient _relay;
  final SharedPreferences _prefs;
  final _recorder = AudioRecorder();
  final RecentDictations recents;

  /// Input level, 0–1, while recording.
  final level = ValueNotifier<double>(0);
  final elapsed = ValueNotifier(Duration.zero);

  var _phase = DictatePhase.idle;
  var _starting = false;
  var _paused = false;
  var _cancelling = false;
  var _disposed = false;
  String? _stage;
  DictateOutcome? _outcome;
  RelayRequest? _request;
  String? _recordingPath;

  /// The computer a recording or request is for, from its start to its
  /// result, whatever happens to the account's computers meanwhile.
  Device? _target;
  final _stopwatch = Stopwatch();
  StreamSubscription<Amplitude>? _amplitude;
  StreamSubscription<RecordState>? _recorderState;
  Timer? _limit;

  /// A recording kept after a failed send, for Try again.
  ({String path, Duration length})? _kept;

  /// The window input goes to, and the computer it is on. Not remembered
  /// across runs: its id expires after 30 minutes anyway.
  ({String computer, RemoteWindow window})? _window;

  /// The last request's window closed or its id expired, so Try again waits
  /// until the user chooses again (it would otherwise go to the cursor).
  var _windowLost = false;
  var _keysPending = 0;
  Timer? _enterOffer;

  DictatePhase get phase => _phase;

  /// Recording, but paused by a call or another app's audio; it resumes on
  /// its own when that ends.
  bool get paused => _paused;

  /// The computer's last `progress` stage while sending.
  String? get stage => _stage;
  bool get cancelling => _cancelling;
  DictateOutcome? get outcome => _outcome;
  bool get canRetry =>
      _kept != null && _phase == DictatePhase.idle && !_windowLost;

  /// Whether the computer lets this phone choose its window and press keys.
  bool get hasWindows =>
      computer?.capabilities.contains(WindowsKind.capability) ?? false;

  /// The window input goes to; null for wherever the cursor is.
  RemoteWindow? get window {
    final window = _window;
    if (window == null || !hasWindows) return null;
    return window.computer == computer?.id ? window.window : null;
  }

  /// The last request failed because its window is gone.
  bool get windowLost => _windowLost;

  /// Whether the "Press Enter" chip shows: a paste just finished.
  bool get offersEnter => _enterOffer != null;

  List<Device> get computers => computersOf(_relay.devices.value ?? const []);
  Device? get computer {
    final computers = this.computers;
    if (_target case final target?) {
      return computers.where((d) => d.id == target.id).firstOrNull ?? target;
    }
    final chosen = _prefs.getString(_chosenKey);
    final computer = pickComputer(computers, chosen);
    // An automatic pick is remembered like a choice, so a computer that
    // joined later is not inherited once this one leaves the account.
    if (chosen == null && computer != null) {
      unawaited(_prefs.setString(_chosenKey, computer.id));
    }
    return computer;
  }

  void choose(String deviceId) {
    unawaited(_prefs.setString(_chosenKey, deviceId));
    _window = null;
    _windowLost = false;
    _clearEnterOffer();
    _notify();
  }

  /// Aims what is sent next at [window]; null for wherever the cursor is.
  void chooseWindow(RemoteWindow? window) {
    final computer = this.computer;
    _window = window == null || computer == null
        ? null
        : (computer: computer.id, window: window);
    _windowLost = false;
    _clearEnterOffer();
    _notify();
  }

  /// Lists the computer's windows (and its screens, from a computer with
  /// live view), or answers an error code. A new list ends the ids of the
  /// last one, so the chosen window moves to its new id, or is forgotten
  /// when the list does not have it exactly once.
  Future<
    ({List<RemoteWindow> windows, List<RemoteScreen> screens, String? error})
  >
  listWindows() async {
    final computer = this.computer;
    if (computer == null) {
      return (
        windows: const <RemoteWindow>[],
        screens: const <RemoteScreen>[],
        error: 'offline',
      );
    }
    final reply = await _ask(computer, {
      'kind': WindowsKind.list,
    }, timeout: _windowsTimeout);
    if (reply.error case final error?) {
      return (
        windows: const <RemoteWindow>[],
        screens: const <RemoteScreen>[],
        error: error,
      );
    }
    final windows = windowsOf(reply.result!);
    final screens = computer.capabilities.contains(ScreenKind.capability)
        ? screensOf(reply.result!)
        : const <RemoteScreen>[];
    if (_window case (
      computer: final id,
      :final window,
    ) when id == computer.id) {
      final same = windows.where(window.sameAs).toList();
      _window = same.length == 1
          ? (computer: computer.id, window: same.single)
          : null;
      _notify();
    }
    return (windows: windows, screens: screens, error: null);
  }

  /// Presses [keys] (combos) in the chosen window. Null when they were
  /// pressed, or the tap was ignored because [maxPendingKeys] still wait;
  /// otherwise an error code.
  Future<String?> pressKeys(List<String> keys) async {
    final computer = this.computer;
    if (computer == null) return 'offline';
    if (_keysPending >= maxPendingKeys) return null;
    final window = this.window;
    _keysPending++;
    try {
      final reply = await _ask(computer, {
        'kind': WindowsKind.keys,
        'keys': keys,
        ..._aim(window),
      }, timeout: _windowsTimeout);
      if (reply.error == 'target_gone') _loseWindow(window);
      return reply.error;
    } finally {
      _keysPending--;
    }
  }

  /// Presses Enter where the last paste went.
  Future<String?> pressEnter() {
    _clearEnterOffer();
    _notify();
    return pressKeys(const ['enter']);
  }

  Deliver get deliver =>
      _prefs.getString(_deliverKey) == 'return' ? Deliver.keep : Deliver.paste;

  set deliver(Deliver value) {
    unawaited(
      _prefs.setString(_deliverKey, value == Deliver.keep ? 'return' : 'paste'),
    );
    _notify();
  }

  Future<void> startRecording() async {
    if (_phase != DictatePhase.idle || _starting) return;
    _starting = true;
    _outcome = null;
    await _dropKept();
    _notify();
    try {
      if (!await _recorder.hasPermission()) {
        _outcome = const DictateOutcome.failed('mic_denied');
        return;
      }
      final dir = await getTemporaryDirectory();
      final path =
          '${dir.path}/$_filePrefix${DateTime.now().millisecondsSinceEpoch}.wav';
      await _recorder.start(
        const RecordConfig(
          encoder: AudioEncoder.wav,
          sampleRate: 16000,
          numChannels: 1,
          // The default pauses on any interruption (a call, a notification
          // sound) and never resumes, while this screen says it records.
          audioInterruption: AudioInterruptionMode.pauseResume,
        ),
        path: path,
      );
      _recordingPath = path;
      _target = computer;
      _phase = DictatePhase.recording;
      level.value = 0;
      elapsed.value = Duration.zero;
      _stopwatch
        ..reset()
        ..start();
      _amplitude = _recorder
          .onAmplitudeChanged(const Duration(milliseconds: 100))
          .listen((amplitude) {
            // dBFS: treat -50 dB and below as silence.
            level.value = ((amplitude.current + 50) / 50).clamp(0.0, 1.0);
            elapsed.value = _stopwatch.elapsed;
          });
      _recorderState = _recorder.onStateChanged().listen(_onRecorderState);
      _limit = Timer(maxRecording, stopAndSend);
    } on Exception {
      _outcome = const DictateOutcome.failed('mic_failed');
    } finally {
      _starting = false;
      _notify();
    }
  }

  /// Stops and sends the recording, delivered as [deliverAs] when given
  /// (live view always pastes), else as [deliver].
  Future<void> stopAndSend({Deliver? deliverAs}) async {
    if (_phase != DictatePhase.recording) return;
    final length = _endRecording();
    String? path;
    try {
      path = await _recorder.stop();
    } on Exception {
      path = null;
    }
    _recordingPath = null;
    _phase = DictatePhase.idle;
    if (path == null) {
      _finish(const DictateOutcome.failed('mic_failed'));
      return;
    }
    _kept = (path: path, length: length);
    await retry(deliverAs: deliverAs);
  }

  Future<void> discardRecording() async {
    if (_phase != DictatePhase.recording) return;
    _endRecording();
    _recordingPath = null;
    _target = null;
    _phase = DictatePhase.idle;
    _notify();
    await _recorder.cancel(); // also deletes the file
  }

  /// Sends the kept recording (again), delivered as [deliverAs] when given.
  Future<void> retry({Deliver? deliverAs}) async {
    final kept = _kept;
    final computer = this.computer;
    if (kept == null || _phase != DictatePhase.idle) return;
    if (computer == null) {
      _finish(const DictateOutcome.failed('offline'));
      return;
    }
    _target = computer;
    _phase = DictatePhase.sending;
    _stage = 'transcribing';
    _outcome = null;
    _notify();

    final String audio;
    try {
      audio = await Isolate.run(
        () => base64Encode(File(kept.path).readAsBytesSync()),
      );
    } on FileSystemException {
      _kept = null;
      _finish(const DictateOutcome.failed('mic_failed'));
      return;
    }
    final window = this.window;
    final outcome = await _send(
      computer,
      {
        'kind': DictateKind.dictate,
        'deliver': (deliverAs ?? deliver) == Deliver.keep ? 'return' : 'paste',
        'audio': audio,
        ..._aim(window),
      },
      timeout: _resultTimeout + kept.length,
      window: window,
    );

    if (outcome.error == null || _final.contains(outcome.error)) {
      await _dropKept();
    }
    if (outcome.text case final text? when text.isNotEmpty) {
      await recents.add(text);
    }
    _finish(outcome);
  }

  /// Pastes [text] on the computer. True when it was pasted.
  Future<bool> sendText(String text) async {
    final computer = this.computer;
    if (computer == null || _phase != DictatePhase.idle) return false;
    _target = computer;
    _phase = DictatePhase.sending;
    _stage = 'pasting';
    _outcome = null;
    await _dropKept();
    _notify();
    final window = this.window;
    final outcome = await _send(
      computer,
      {'kind': DictateKind.text, 'text': text, ..._aim(window)},
      timeout: _resultTimeout,
      typed: true,
      window: window,
    );
    _finish(outcome);
    return outcome.error == null;
  }

  /// Asks the computer to stop; its answer still decides the outcome.
  void cancelRequest() {
    final request = _request;
    if (request == null || _cancelling) return;
    _cancelling = true;
    _relay.cancel(request.id);
    _notify();
  }

  void dismissOutcome() {
    _outcome = null;
    _windowLost = false;
    _clearEnterOffer();
    unawaited(_dropKept());
    _notify();
  }

  Future<DictateOutcome> _send(
    Device computer,
    Map<String, Object?> body, {
    required Duration timeout,
    bool typed = false,
    RemoteWindow? window,
  }) async {
    final reply = await _ask(
      computer,
      body,
      timeout: timeout,
      onRequest: (request) => _request = request,
      onStage: (stage) {
        _stage = stage;
        _notify();
      },
    );
    _request = null;
    _cancelling = false;
    if (reply.error == 'target_gone') _loseWindow(window);
    return switch (reply) {
      (result: final result?, error: null) => DictateOutcome.done(
        computer: computer.name,
        text: result['text'] is String ? result['text']! as String : null,
        pasted: result['pasted'] == true,
        typed: typed,
        window: window,
      ),
      (:final error, result: _) => DictateOutcome.failed(
        error ?? 'unknown',
        computer: computer.name,
        typed: typed,
      ),
    };
  }

  /// Sends [body] to [computer] and waits for its result: the result when
  /// it is `ok`, else an error code (the computer's, or the phone's own:
  /// offline, not_connected, interrupted, timeout).
  Future<({Map<String, Object?>? result, String? error})> _ask(
    Device computer,
    Map<String, Object?> body, {
    required Duration timeout,
    void Function(RelayRequest request)? onRequest,
    void Function(String stage)? onStage,
  }) async {
    final request = _relay.request(computer.id, body);
    onRequest?.call(request);
    final result = Completer<Map<String, Object?>>();
    Timer? timer;
    // Every progress starts the wait again: the computer is still at it.
    void wait(Duration limit) {
      timer?.cancel();
      timer = Timer(limit, () {
        _relay.cancel(request.id);
        if (!result.isCompleted) result.completeError(const _TimedOut());
      });
    }

    wait(timeout);
    final subscription = request.replies.listen(
      (reply) {
        if (reply['kind'] == RelayKind.progress) {
          if (reply['stage'] case final String stage) onStage?.call(stage);
          wait(reply['stage'] == 'approval' ? _approvalTimeout : timeout);
        } else if (reply['kind'] == RelayKind.result && !result.isCompleted) {
          result.complete(reply);
        }
      },
      onError: (Object error) {
        if (!result.isCompleted) result.completeError(error);
      },
    );

    try {
      return switch (await result.future) {
        {'ok': true} && final reply => (result: reply, error: null),
        final reply => (
          result: null,
          error: reply['error'] is String
              ? reply['error']! as String
              : 'unknown',
        ),
      };
    } on RelayException catch (e) {
      return (
        result: null,
        error: switch (e.error) {
          RelayError.offline => 'offline',
          RelayError.notConnected => 'not_connected',
          RelayError.interrupted => 'interrupted',
        },
      );
    } on _TimedOut {
      return (result: null, error: 'timeout');
    } finally {
      timer?.cancel();
      unawaited(subscription.cancel());
    }
  }

  /// The fields that aim a request at [window]; none for the cursor.
  static Map<String, Object?> _aim(RemoteWindow? window) =>
      window == null ? const {} : {'target': window.id, 'stay': false};

  /// [window] closed or its id expired: forget it, unless another one was
  /// chosen meanwhile.
  void _loseWindow(RemoteWindow? window) {
    if (window == null || _window?.window.id != window.id) return;
    _window = null;
    _windowLost = true;
    _notify();
  }

  void _clearEnterOffer() {
    _enterOffer?.cancel();
    _enterOffer = null;
  }

  void _finish(DictateOutcome outcome) {
    _target = null;
    _phase = DictatePhase.idle;
    _stage = null;
    _outcome = outcome;
    _clearEnterOffer();
    // The computer never presses Enter after a phone's paste (PROTOCOL.md).
    if (outcome.pasted || (outcome.typed && outcome.error == null)) {
      _enterOffer = Timer(enterOfferFor, () {
        _enterOffer = null;
        _notify();
      });
    }
    _notify();
  }

  /// Nothing here pauses the recorder, so a pause is an interruption.
  void _onRecorderState(RecordState state) {
    final paused = state == RecordState.pause;
    if (_phase != DictatePhase.recording || paused == _paused) return;
    _paused = paused;
    if (paused) {
      _stopwatch.stop();
      level.value = 0;
    } else {
      _stopwatch.start();
    }
    _notify();
  }

  Duration _endRecording() {
    _limit?.cancel();
    _limit = null;
    _amplitude?.cancel();
    _amplitude = null;
    _recorderState?.cancel();
    _recorderState = null;
    _paused = false;
    _stopwatch.stop();
    level.value = 0;
    return _stopwatch.elapsed;
  }

  Future<void> _dropKept() async {
    final kept = _kept;
    _kept = null;
    if (kept != null) await _delete(kept.path);
  }

  /// Recordings left behind by a run that was killed mid-way.
  Future<void> _sweepRecordings() async {
    try {
      final dir = await getTemporaryDirectory();
      await for (final entry in dir.list()) {
        if (entry is File &&
            entry.uri.pathSegments.last.startsWith(_filePrefix) &&
            entry.path != _recordingPath &&
            entry.path != _kept?.path) {
          await _delete(entry.path);
        }
      }
    } on FileSystemException {
      // The OS clears its temporary directory on its own.
    }
  }

  static Future<void> _delete(String path) async {
    try {
      await File(path).delete();
    } on FileSystemException {
      // Already gone.
    }
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _clearEnterOffer();
    _endRecording();
    unawaited(_recorder.dispose());
    if (_recordingPath case final path?) unawaited(_delete(path));
    unawaited(_dropKept());
    level.dispose();
    elapsed.dispose();
    super.dispose();
  }
}

class _TimedOut implements Exception {
  const _TimedOut();
}
