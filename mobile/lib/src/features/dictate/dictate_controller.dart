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
import 'recent_dictations.dart';

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
  }) : error = null;

  const DictateOutcome.failed(
    String this.error, {
    this.computer,
    this.typed = false,
  }) : text = null,
       pasted = false;

  final String? computer;
  final String? text;
  final bool pasted;
  final String? error;

  /// Typed text rather than a recording.
  final bool typed;
}

/// The protocol allows 5 minutes of audio; stopping a second early keeps the
/// recorder's start and stop latency from pushing a full recording over.
const maxRecording = Duration(minutes: 4, seconds: 59);

/// The wait for a result beyond the recording's own length (PROTOCOL.md).
const _resultTimeout = Duration(seconds: 60);

/// The wait while the computer asks its user to allow this phone; its own
/// prompt gives up after 2 minutes (PROTOCOL.md, Approval).
const _approvalTimeout = Duration(minutes: 3);

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

  DictatePhase get phase => _phase;

  /// Recording, but paused by a call or another app's audio; it resumes on
  /// its own when that ends.
  bool get paused => _paused;

  /// The computer's last `progress` stage while sending.
  String? get stage => _stage;
  bool get cancelling => _cancelling;
  DictateOutcome? get outcome => _outcome;
  bool get canRetry => _kept != null && _phase == DictatePhase.idle;

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
    _notify();
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

  Future<void> stopAndSend() async {
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
    await retry();
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

  /// Sends the kept recording (again).
  Future<void> retry() async {
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
    final outcome = await _send(computer, {
      'kind': DictateKind.dictate,
      'deliver': deliver == Deliver.keep ? 'return' : 'paste',
      'audio': audio,
    }, timeout: _resultTimeout + kept.length);

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
    final outcome = await _send(
      computer,
      {'kind': DictateKind.text, 'text': text},
      timeout: _resultTimeout,
      typed: true,
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
    unawaited(_dropKept());
    _notify();
  }

  Future<DictateOutcome> _send(
    Device computer,
    Map<String, Object?> body, {
    required Duration timeout,
    bool typed = false,
  }) async {
    final request = _relay.request(computer.id, body);
    _request = request;
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
          if (reply['stage'] case final String stage) _stage = stage;
          wait(reply['stage'] == 'approval' ? _approvalTimeout : timeout);
          _notify();
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
        {'ok': true} && final reply => DictateOutcome.done(
          computer: computer.name,
          text: reply['text'] is String ? reply['text']! as String : null,
          pasted: reply['pasted'] == true,
          typed: typed,
        ),
        final reply => DictateOutcome.failed(
          reply['error'] is String ? reply['error']! as String : 'unknown',
          computer: computer.name,
          typed: typed,
        ),
      };
    } on RelayException catch (e) {
      return DictateOutcome.failed(
        switch (e.error) {
          RelayError.offline => 'offline',
          RelayError.notConnected => 'not_connected',
          RelayError.interrupted => 'interrupted',
        },
        computer: computer.name,
        typed: typed,
      );
    } on _TimedOut {
      return DictateOutcome.failed(
        'timeout',
        computer: computer.name,
        typed: typed,
      );
    } finally {
      timer?.cancel();
      unawaited(subscription.cancel());
      _request = null;
      _cancelling = false;
    }
  }

  void _finish(DictateOutcome outcome) {
    _target = null;
    _phase = DictatePhase.idle;
    _stage = null;
    _outcome = outcome;
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
