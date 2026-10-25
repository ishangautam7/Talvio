import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart' show MissingPluginException;
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

/// The protocol allows 5 minutes of audio; stopping a second early keeps the
/// recorder's start and stop latency from pushing a full recording over.
const maxRecording = Duration(minutes: 4, seconds: 59);

/// One recording at a time from the microphone, as the computer wants it:
/// a 16 kHz mono 16-bit WAV in the temporary directory, named with
/// [filePrefix] so a run that was killed mid-way can be swept up later.
/// Shared by every page that records (Send, Do).
class VoiceRecorder {
  VoiceRecorder({required this.filePrefix, required this.onChanged});

  final String filePrefix;

  /// Called when [paused] changes.
  final VoidCallback onChanged;

  /// The recorder holding the microphone, from [start] to the recording's
  /// end: one page records at a time.
  static final ValueNotifier<VoiceRecorder?> holder = ValueNotifier(null);
  static VoiceRecorder? get _holder => holder.value;
  static set _holder(VoiceRecorder? value) => holder.value = value;

  /// Whether a recorder other than [self] holds the microphone.
  static bool heldByOther(VoiceRecorder self) =>
      _holder != null && !identical(_holder, self);

  AudioRecorder? _recorder;
  AudioRecorder get _audio => _recorder ??= AudioRecorder();

  /// Input level, 0–1, while recording.
  final level = ValueNotifier<double>(0);
  final elapsed = ValueNotifier(Duration.zero);

  var _starting = false;
  var _recording = false;
  var _paused = false;
  String? _path;
  final _stopwatch = Stopwatch();
  StreamSubscription<Amplitude>? _amplitude;
  StreamSubscription<RecordState>? _state;
  Timer? _limit;

  /// Between [start] being called and it answering.
  bool get starting => _starting;
  bool get recording => _recording;

  /// Recording, but paused by a call or another app's audio; it resumes on
  /// its own when that ends.
  bool get paused => _paused;

  /// The file being recorded into, while recording.
  String? get path => _path;

  /// Starts recording. Answers null when it started, else an error code:
  /// `mic_denied`, `mic_failed`, or `mic_busy` while another page records.
  /// [onLimit] runs when the recording reaches [maxRecording].
  Future<String?> start({required VoidCallback onLimit}) async {
    if (_recording || _starting) return null;
    if (heldByOther(this)) return 'mic_busy';
    _starting = true;
    _holder = this;
    var started = false;
    try {
      if (!await _audio.hasPermission()) return 'mic_denied';
      final dir = await getTemporaryDirectory();
      final path =
          '${dir.path}/$filePrefix${DateTime.now().millisecondsSinceEpoch}.wav';
      await _audio.start(
        const RecordConfig(
          encoder: AudioEncoder.wav,
          sampleRate: 16000,
          numChannels: 1,
          // The default pauses on any interruption (a call, a notification
          // sound) and never resumes, while the page says it records.
          audioInterruption: AudioInterruptionMode.pauseResume,
        ),
        path: path,
      );
      _path = path;
      _recording = true;
      level.value = 0;
      elapsed.value = Duration.zero;
      _stopwatch
        ..reset()
        ..start();
      _amplitude = _audio
          .onAmplitudeChanged(const Duration(milliseconds: 100))
          .listen((amplitude) {
            // dBFS: treat -50 dB and below as silence.
            level.value = ((amplitude.current + 50) / 50).clamp(0.0, 1.0);
            elapsed.value = _stopwatch.elapsed;
          });
      _state = _audio.onStateChanged().listen(_onState);
      _limit = Timer(maxRecording, onLimit);
      started = true;
      return null;
    } on Exception {
      return 'mic_failed';
    } finally {
      _starting = false;
      if (!started) _release();
    }
  }

  /// Stops and answers the file and its length; null when the recorder
  /// failed (or nothing was recording).
  Future<({String path, Duration length})?> stop() async {
    if (!_recording) return null;
    final length = _end();
    String? path;
    try {
      path = await _audio.stop();
    } on Exception {
      path = null;
    }
    _path = null;
    return path == null ? null : (path: path, length: length);
  }

  /// Stops and throws the recording away.
  Future<void> discard() async {
    if (!_recording) return;
    _end();
    _path = null;
    await _audio.cancel(); // also deletes the file
  }

  /// Deletes recordings of [filePrefix] left behind by a run that was killed
  /// mid-way, except the one recording now and [keep].
  Future<void> sweep({String? keep}) async {
    try {
      final dir = await getTemporaryDirectory();
      await for (final entry in dir.list()) {
        if (entry is File &&
            entry.uri.pathSegments.last.startsWith(filePrefix) &&
            entry.path != _path &&
            entry.path != keep) {
          await delete(entry.path);
        }
      }
    } on FileSystemException {
      // The OS clears its temporary directory on its own.
    } on MissingPluginException {
      // No path provider (tests).
    }
  }

  static Future<void> delete(String path) async {
    try {
      await File(path).delete();
    } on FileSystemException {
      // Already gone.
    }
  }

  /// Nothing here pauses the recorder, so a pause is an interruption.
  void _onState(RecordState state) {
    final paused = state == RecordState.pause;
    if (!_recording || paused == _paused) return;
    _paused = paused;
    if (paused) {
      _stopwatch.stop();
      level.value = 0;
    } else {
      _stopwatch.start();
    }
    onChanged();
  }

  void _release() {
    if (identical(_holder, this)) _holder = null;
  }

  Duration _end() {
    _release();
    _recording = false;
    _limit?.cancel();
    _limit = null;
    _amplitude?.cancel();
    _amplitude = null;
    _state?.cancel();
    _state = null;
    _paused = false;
    _stopwatch.stop();
    level.value = 0;
    return _stopwatch.elapsed;
  }

  void dispose() {
    _end();
    if (_recorder case final recorder?) unawaited(recorder.dispose());
    if (_path case final path?) unawaited(delete(path));
    level.dispose();
    elapsed.dispose();
  }
}
