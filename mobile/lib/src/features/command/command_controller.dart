import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../core/models.dart';
import '../../core/relay_ask.dart';
import '../../core/relay_client.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/voice_recorder.dart';
import 'recent_commands.dart';

/// The capability and request kinds of commands (PROTOCOL.md, "Commands").
abstract final class CommandKind {
  static const capability = 'command';
  static const command = 'command';
  static const confirm = 'command.confirm';
  static const answer = 'command.answer';
}

/// A command result's `status`.
abstract final class CommandStatus {
  static const done = 'done';
  static const awaitingConfirmation = 'awaiting_confirmation';
  static const needsAnswer = 'needs_answer';
  static const cancelled = 'cancelled';
  static const failed = 'failed';
  static const needsComputerUse = 'needs_computer_use';
}

/// The wait for a command's result, started again by every `progress`;
/// a recording adds its own length (PROTOCOL.md).
const commandTimeout = Duration(seconds: 45);

/// Commands are at most 2 000 characters (PROTOCOL.md).
const maxCommandChars = 2000;

/// What the computer did with a command: an `ok: true` result. The computer
/// is a trust boundary, so a field of the wrong type reads as absent.
class CommandResult {
  const CommandResult({
    required this.status,
    required this.feedback,
    this.transcript,
    this.id,
    this.pending,
    this.question,
    this.choices = const [],
  });

  factory CommandResult.parse(Map<String, Object?> json) {
    String? text(String key) => switch (json[key]) {
      final String value when value.trim().isNotEmpty => value.trim(),
      _ => null,
    };
    return CommandResult(
      status: text('status') ?? CommandStatus.done,
      feedback: text('feedback') ?? '',
      transcript: text('transcript'),
      id: json['id'] is int ? json['id']! as int : null,
      pending: text('pending'),
      question: text('question'),
      choices: [
        if (json['choices'] case final List<Object?> choices)
          for (final choice in choices)
            if (choice is String && choice.trim().isNotEmpty) choice.trim(),
      ],
    );
  }

  final String status;

  /// One or two short sentences, in English.
  final String feedback;
  final String? transcript;

  /// What to confirm or answer, with those statuses.
  final int? id;

  /// What accepting does, with `awaiting_confirmation`.
  final String? pending;
  final String? question;
  final List<String> choices;

  /// The command waits for Do it / Cancel.
  bool get confirmable =>
      status == CommandStatus.awaitingConfirmation && id != null;

  /// The command waits for an answer.
  bool get answerable => status == CommandStatus.needsAnswer && id != null;
}

/// How the last request ended: a [result], or an [error] code (the
/// computer's, or the phone's own: offline, not_connected, interrupted,
/// timeout, mic_denied, mic_failed).
class CommandOutcome {
  const CommandOutcome.result(
    CommandResult this.result, {
    required this.computer,
    this.said,
  }) : error = null;

  const CommandOutcome.failed(
    String this.error, {
    required this.computer,
    this.said,
  }) : result = null;

  final CommandResult? result;
  final String? error;
  final String computer;

  /// What the user said or typed, when known.
  final String? said;
}

enum CommandPhase { idle, recording, sending }

/// Errors a resend of the same command cannot fix.
const _final = {'bad_audio', 'no_speech', 'unsupported'};

/// Commands for the computer the Send page has chosen ([DictateController]
/// owns that choice): spoken or typed, then the confirmation or answer a
/// result may ask for. One request at a time.
class CommandController extends ChangeNotifier {
  CommandController({
    required this._relay,
    required SharedPreferences prefs,
    required this._dictate,
  }) : recents = RecentCommands(prefs) {
    _voice = VoiceRecorder(filePrefix: _filePrefix, onChanged: _notify);
    _dictate.addListener(_notify);
    unawaited(_voice.sweep());
  }

  static const _filePrefix = 'command-';

  final RelayClient _relay;
  final DictateController _dictate;
  late final VoiceRecorder _voice;
  final RecentCommands recents;

  var _phase = CommandPhase.idle;
  var _cancelling = false;
  var _disposed = false;
  String? _stage;
  CommandOutcome? _outcome;
  RelayRequest? _request;

  /// The computer a recording or request is for, to its end.
  Device? _target;

  /// The last `command` request, for Try again.
  ({Map<String, Object?> body, Duration timeout, String? said})? _last;

  CommandPhase get phase => _phase;
  bool get paused => _voice.paused;
  ValueListenable<double> get level => _voice.level;
  ValueListenable<Duration> get elapsed => _voice.elapsed;

  /// The computer's last `progress` stage while sending; null before one.
  String? get stage => _stage;
  bool get cancelling => _cancelling;
  CommandOutcome? get outcome => _outcome;

  /// Whether the last failed command can be sent again.
  bool get canRetry =>
      _last != null &&
      _phase == CommandPhase.idle &&
      _outcome?.error != null &&
      !_final.contains(_outcome?.error);

  /// The chosen computer, or the one a request is for.
  Device? get computer {
    if (_target case final target?) {
      return _dictate.computers.where((d) => d.id == target.id).firstOrNull ??
          target;
    }
    return _dictate.computer;
  }

  /// Whether the user may switch computers now.
  bool get idle =>
      _phase == CommandPhase.idle && _dictate.phase == DictatePhase.idle;

  Future<void> startRecording() async {
    // One microphone: not while the Send page records or sends.
    if (!idle || _voice.starting) return;
    _outcome = null;
    _notify();
    final error = await _voice.start(onLimit: stopAndSend);
    if (_disposed) return;
    if (error != null) {
      _outcome = CommandOutcome.failed(error, computer: computer?.name ?? '');
    } else {
      _target = computer;
      _phase = CommandPhase.recording;
    }
    _notify();
  }

  Future<void> stopAndSend() async {
    if (_phase != CommandPhase.recording) return;
    final recorded = await _voice.stop();
    final computer = this.computer;
    _phase = CommandPhase.idle;
    if (recorded == null || computer == null) {
      _finish(
        CommandOutcome.failed(
          recorded == null ? 'mic_failed' : 'offline',
          computer: computer?.name ?? '',
        ),
      );
      return;
    }
    _phase = CommandPhase.sending;
    _stage = 'transcribing';
    _notify();
    final String audio;
    try {
      audio = await Isolate.run(
        () => base64Encode(File(recorded.path).readAsBytesSync()),
      );
    } on FileSystemException {
      _finish(CommandOutcome.failed('mic_failed', computer: computer.name));
      return;
    } finally {
      unawaited(VoiceRecorder.delete(recorded.path));
    }
    _phase = CommandPhase.idle;
    await _command(computer, {
      'kind': CommandKind.command,
      'audio': audio,
    }, timeout: commandTimeout + recorded.length);
  }

  Future<void> discardRecording() async {
    if (_phase != CommandPhase.recording) return;
    _target = null;
    _phase = CommandPhase.idle;
    _notify();
    await _voice.discard();
  }

  /// Sends a typed command. True when the computer took it (whatever it
  /// then managed to do).
  Future<bool> sendText(String text) async {
    final command = text.trim();
    final computer = this.computer;
    if (command.isEmpty || computer == null || !idle) return false;
    await _command(
      computer,
      {'kind': CommandKind.command, 'text': _clip(command)},
      timeout: commandTimeout,
      said: command,
    );
    return _outcome?.error == null;
  }

  /// Sends the last command again.
  Future<void> retry() async {
    final last = _last;
    final computer = this.computer;
    if (last == null || computer == null || !idle) return;
    await _command(computer, last.body, timeout: last.timeout, said: last.said);
  }

  /// Do it ([accept]) or Cancel for a result awaiting confirmation.
  Future<void> confirm({required bool accept}) async {
    final result = _outcome?.result;
    if (result == null || !result.confirmable) return;
    await _followUp({
      'kind': CommandKind.confirm,
      'id': result.id,
      'accept': accept,
    });
  }

  /// Answers a result's question.
  Future<void> answer(String answer) async {
    final result = _outcome?.result;
    final text = answer.trim();
    if (result == null || !result.answerable || text.isEmpty) return;
    await _followUp({
      'kind': CommandKind.answer,
      'id': result.id,
      'answer': _clip(text),
    });
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
    _last = null;
    _notify();
  }

  Future<void> _command(
    Device computer,
    Map<String, Object?> body, {
    required Duration timeout,
    String? said,
  }) async {
    _last = (body: body, timeout: timeout, said: said);
    final reply = await _send(computer, body, timeout: timeout);
    final error = reply.error;
    if (error != null) {
      _finish(
        CommandOutcome.failed(error, computer: computer.name, said: said),
      );
      return;
    }
    final result = CommandResult.parse(reply.result!);
    final spoken = result.transcript ?? said;
    _last = null;
    if (spoken != null) {
      await recents.add(
        RecentCommand(
          said: spoken,
          feedback: result.feedback,
          status: result.status,
          id: result.id,
          at: DateTime.now(),
        ),
      );
    }
    _finish(
      CommandOutcome.result(result, computer: computer.name, said: spoken),
    );
  }

  /// A confirmation or an answer for the result on screen; its result
  /// replaces that one (and its entry in [recents]).
  Future<void> _followUp(Map<String, Object?> body) async {
    final computer = this.computer;
    final previous = _outcome;
    if (computer == null || previous == null || !idle) return;
    _outcome = null;
    _last = null;
    final reply = await _send(computer, body, timeout: commandTimeout);
    final said = previous.said;
    if (reply.error case final error?) {
      _finish(
        CommandOutcome.failed(error, computer: computer.name, said: said),
      );
      return;
    }
    final result = CommandResult.parse(reply.result!);
    await recents.update(
      body['id']! as int,
      feedback: result.feedback,
      status: result.status,
      id: result.id,
    );
    _finish(
      CommandOutcome.result(
        result,
        computer: computer.name,
        said: result.transcript ?? said,
      ),
    );
  }

  Future<RelayAnswer> _send(
    Device computer,
    Map<String, Object?> body, {
    required Duration timeout,
  }) async {
    _target = computer;
    _phase = CommandPhase.sending;
    _stage = body['audio'] != null ? 'transcribing' : null;
    _outcome = null;
    _notify();
    final reply = await askComputer(
      _relay,
      computer.id,
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
    return reply;
  }

  void _finish(CommandOutcome outcome) {
    _target = null;
    _phase = CommandPhase.idle;
    _stage = null;
    _outcome = outcome;
    _notify();
  }

  /// At most [maxCommandChars], counted in code points as the computer
  /// counts them.
  static String _clip(String text) => text.runes.length <= maxCommandChars
      ? text
      : String.fromCharCodes(text.runes.take(maxCommandChars));

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _dictate.removeListener(_notify);
    _voice.dispose();
    super.dispose();
  }
}
