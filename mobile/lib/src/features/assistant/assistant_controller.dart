import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../core/models.dart';
import '../../core/relay_ask.dart';
import '../../core/relay_client.dart';
import '../../core/session_store.dart';
import '../command/command_controller.dart';
import '../dictate/dictate_controller.dart';
import '../dictate/voice_recorder.dart';

/// The capability and request kind of the assistant (PROTOCOL.md,
/// "Assistant").
abstract final class AssistantKind {
  static const capability = 'assistant';
  static const ask = 'assistant.ask';
}

/// The wait for an answer, started again by every `progress`; a recording
/// adds its own length (PROTOCOL.md).
const assistantTimeout = Duration(seconds: 60);

/// Questions are at most 4 000 characters (PROTOCOL.md).
const maxQuestionChars = 4000;

/// Whether [computer] has the assistant.
bool hasAssistant(Device? computer) =>
    computer?.capabilities.contains(AssistantKind.capability) ?? false;

/// One thing the assistant did on the computer.
class AssistantAction {
  const AssistantAction({required this.label, required this.status});

  static AssistantAction? tryParse(Object? json) => switch (json) {
    {'label': String label} when label.trim().isNotEmpty => AssistantAction(
      label: label.trim(),
      status: json['status'] is String ? json['status']! as String : 'done',
    ),
    _ => null,
  };

  final String label;

  /// A command's status: done, failed, awaiting_confirmation, …
  final String status;

  Map<String, Object?> toJson() => {'label': label, 'status': status};
}

/// An action waiting for Do it, confirmed with `command.confirm`.
class AssistantPending {
  const AssistantPending({required this.id, required this.summary});

  static AssistantPending? tryParse(Object? json) => switch (json) {
    {'id': int id, 'summary': String summary} => AssistantPending(
      id: id,
      summary: summary.trim(),
    ),
    _ => null,
  };

  final int id;

  /// What Do it does.
  final String summary;

  Map<String, Object?> toJson() => {'id': id, 'summary': summary};
}

/// One question and its answer. Mutable while it is answered; the
/// controller is the only writer.
class AssistantTurn {
  AssistantTurn({
    required this.computerId,
    required this.computerName,
    required this.spoken,
    this.question,
  });

  final String computerId;
  final String computerName;

  /// Asked by voice.
  final bool spoken;

  /// As typed, or as the computer transcribed it; null until a spoken
  /// question is transcribed.
  String? question;

  /// Markdown. While answering, the answer so far.
  String answer = '';

  /// The computer's last `progress` stage while answering; null before one.
  String? stage;

  /// The tool at work, with stage `tool`.
  String? toolLabel;
  List<AssistantAction> actions = const [];
  AssistantPending? pending;

  /// The computer started a new conversation for this question, though the
  /// phone asked to continue one (the old one expired).
  bool newConversation = false;

  /// The request failed: an error code, and the computer's `message`.
  String? error;
  String? message;
  bool done = false;

  /// A Do it or Cancel for [pending] is on its way.
  bool confirming = false;

  /// How the confirmation ended: a command status, or an error code.
  String? confirmStatus;
  String? confirmFeedback;
  String? confirmError;

  Map<String, Object?> toJson() => {
    'computerId': computerId,
    'computerName': computerName,
    'spoken': spoken,
    'question': ?question,
    'answer': answer,
    'actions': [for (final a in actions) a.toJson()],
    'pending': ?pending?.toJson(),
    if (newConversation) 'newConversation': true,
    'confirmStatus': ?confirmStatus,
    'confirmFeedback': ?confirmFeedback,
  };

  static AssistantTurn? fromJson(Object? json) {
    if (json case {
      'computerId': String computerId,
      'computerName': String computerName,
      'answer': String answer,
    }) {
      return AssistantTurn(
          computerId: computerId,
          computerName: computerName,
          spoken: json['spoken'] == true,
          question: json['question'] is String
              ? json['question']! as String
              : null,
        )
        ..answer = answer
        ..done = true
        ..actions = [
          if (json['actions'] case final List<Object?> list)
            for (final a in list) ?AssistantAction.tryParse(a),
        ]
        ..pending = AssistantPending.tryParse(json['pending'])
        ..newConversation = json['newConversation'] == true
        ..confirmStatus = json['confirmStatus'] is String
            ? json['confirmStatus']! as String
            : null
        ..confirmFeedback = json['confirmFeedback'] is String
            ? json['confirmFeedback']! as String
            : null;
    }
    return null;
  }

  /// Do it and Cancel are offered.
  bool get awaitingConfirmation =>
      pending != null && confirmStatus == null && !confirming;
}

enum AssistantPhase { idle, recording, sending }

/// Errors a resend of the same question cannot fix.
const _final = {'bad_audio', 'no_speech', 'unsupported'};

/// The assistant chat with the computer the Send page has chosen
/// ([DictateController] owns that choice). One question at a time; the
/// conversation continues until New chat. Used by the Assistant page and by
/// live view's assistant bar, so both show the same chat.
class AssistantController extends ChangeNotifier {
  AssistantController({
    required this._relay,
    required this._prefs,
    required this._dictate,
    this._command,
  }) {
    _voice = VoiceRecorder(filePrefix: _filePrefix, onChanged: _notify);
    _dictate.addListener(_notify);
    _command?.addListener(_notify);
    _load();
    unawaited(_voice.sweep());
  }

  static const _filePrefix = 'assistant-';

  /// The chat as last left, under the account prefix so signing out
  /// removes it.
  static const prefsKey = '${accountPrefsPrefix}assistant.chat';

  /// The most turns kept.
  static const limit = 50;

  final RelayClient _relay;
  final SharedPreferences _prefs;
  final DictateController _dictate;
  final CommandController? _command;
  late final VoiceRecorder _voice;

  var _turns = <AssistantTurn>[];
  var _phase = AssistantPhase.idle;
  var _cancelling = false;
  var _disposed = false;
  RelayRequest? _request;

  /// The computer's conversation id, and whose it is.
  String? _conversation;
  String? _conversationComputer;

  /// The computer a recording is for.
  Device? _target;

  /// The last failed question, for Try again.
  ({Map<String, Object?> body, Duration timeout, AssistantTurn turn})? _last;

  /// Oldest first.
  List<AssistantTurn> get turns => _turns;
  AssistantPhase get phase => _phase;
  bool get cancelling => _cancelling;
  bool get paused => _voice.paused;
  ValueListenable<double> get level => _voice.level;
  ValueListenable<Duration> get elapsed => _voice.elapsed;

  /// The conversation the next question continues; null for a new one.
  String? get conversation => _conversation;

  /// The chosen computer, or the one a recording is for.
  Device? get computer {
    if (_target case final target?) {
      return _dictate.computers.where((d) => d.id == target.id).firstOrNull ??
          target;
    }
    return _dictate.computer;
  }

  /// Nothing records or waits on any page.
  bool get idle =>
      _phase == AssistantPhase.idle &&
      _dictate.phase == DictatePhase.idle &&
      (_command?.phase ?? CommandPhase.idle) == CommandPhase.idle;

  /// Whether the last question failed and can be asked again.
  bool get canRetry =>
      _last != null &&
      _phase == AssistantPhase.idle &&
      identical(_last!.turn, _turns.lastOrNull) &&
      !_final.contains(_last!.turn.error);

  /// Asks a typed question. True when it was sent.
  Future<bool> ask(String text) async {
    final question = text.trim();
    final computer = this.computer;
    if (question.isEmpty || computer == null || !idle) return false;
    final turn = AssistantTurn(
      computerId: computer.id,
      computerName: computer.name,
      spoken: false,
      question: _clip(question),
    );
    unawaited(
      _ask(computer, turn, {
        'kind': AssistantKind.ask,
        'text': turn.question,
      }, timeout: assistantTimeout),
    );
    return true;
  }

  Future<void> startRecording() async {
    if (!idle || _voice.starting) return;
    final error = await _voice.start(onLimit: stopAndSend);
    if (_disposed) return;
    if (error != null) {
      final computer = this.computer;
      _add(
        AssistantTurn(
            computerId: computer?.id ?? '',
            computerName: computer?.name ?? '',
            spoken: true,
          )
          ..error = error
          ..done = true,
      );
    } else {
      _target = computer;
      _phase = AssistantPhase.recording;
    }
    _notify();
  }

  Future<void> stopAndSend() async {
    if (_phase != AssistantPhase.recording) return;
    final recorded = await _voice.stop();
    final computer = this.computer;
    _target = null;
    _phase = AssistantPhase.idle;
    final turn = AssistantTurn(
      computerId: computer?.id ?? '',
      computerName: computer?.name ?? '',
      spoken: true,
    );
    if (recorded == null || computer == null) {
      _add(
        turn
          ..error = recorded == null ? 'mic_failed' : 'offline'
          ..done = true,
      );
      _notify();
      return;
    }
    turn.stage = 'transcribing';
    _add(turn);
    _phase = AssistantPhase.sending;
    _notify();
    final String audio;
    try {
      audio = await Isolate.run(
        () => base64Encode(File(recorded.path).readAsBytesSync()),
      );
    } on FileSystemException {
      turn
        ..error = 'mic_failed'
        ..done = true;
      _phase = AssistantPhase.idle;
      _notify();
      return;
    } finally {
      unawaited(VoiceRecorder.delete(recorded.path));
    }
    _phase = AssistantPhase.idle;
    await _ask(
      computer,
      turn,
      {'kind': AssistantKind.ask, 'audio': audio},
      timeout: assistantTimeout + recorded.length,
      added: true,
    );
  }

  Future<void> discardRecording() async {
    if (_phase != AssistantPhase.recording) return;
    _target = null;
    _phase = AssistantPhase.idle;
    _notify();
    await _voice.discard();
  }

  /// Asks the last failed question again, in its place.
  Future<void> retry() async {
    final last = _last;
    final computer = this.computer;
    if (!canRetry || computer == null || !idle) return;
    _turns = [..._turns]..removeLast();
    final turn = AssistantTurn(
      computerId: computer.id,
      computerName: computer.name,
      spoken: last!.turn.spoken,
      question: last.turn.question,
    );
    await _ask(computer, turn, last.body, timeout: last.timeout);
  }

  /// Asks the computer to stop answering; its answer still decides how the
  /// question ends.
  void cancelRequest() {
    final request = _request;
    if (request == null || _cancelling) return;
    _cancelling = true;
    _relay.cancel(request.id);
    _notify();
  }

  /// Do it ([accept]) or Cancel for [turn]'s pending action.
  Future<void> confirm(AssistantTurn turn, {required bool accept}) async {
    final pending = turn.pending;
    final computer = _dictate.computers
        .where((d) => d.id == turn.computerId)
        .firstOrNull;
    if (pending == null || !turn.awaitingConfirmation || computer == null) {
      return;
    }
    turn
      ..confirming = true
      ..confirmError = null;
    _notify();
    final reply = await askComputer(_relay, computer.id, {
      'kind': CommandKind.confirm,
      'id': pending.id,
      'accept': accept,
    }, timeout: commandTimeout);
    if (_disposed) return;
    turn.confirming = false;
    if (reply.error case final error?) {
      turn.confirmError = error;
    } else {
      final result = CommandResult.parse(reply.result!);
      if (result.confirmable) {
        // Waits again, for the next step.
        turn.pending = AssistantPending(
          id: result.id!,
          summary: result.pending ?? pending.summary,
        );
      } else {
        turn
          ..confirmStatus = result.status
          ..confirmFeedback = result.feedback;
      }
    }
    await _save();
    _notify();
  }

  /// Forgets the conversation, here and as the next question's context.
  Future<void> newChat() async {
    if (_phase == AssistantPhase.sending) return;
    _turns = [];
    _conversation = null;
    _conversationComputer = null;
    _last = null;
    await _prefs.remove(prefsKey);
    _notify();
  }

  Future<void> _ask(
    Device computer,
    AssistantTurn turn,
    Map<String, Object?> request, {
    required Duration timeout,
    bool added = false,
  }) async {
    final continuing = _conversationComputer == computer.id
        ? _conversation
        : null;
    final body = {...request, 'conversation': ?continuing};
    if (!added) _add(turn);
    _last = null;
    _phase = AssistantPhase.sending;
    _notify();
    final reply = await askComputer(
      _relay,
      computer.id,
      body,
      timeout: timeout,
      onRequest: (request) => _request = request,
      onProgress: (progress) => _onProgress(turn, progress),
    );
    _request = null;
    _cancelling = false;
    _phase = AssistantPhase.idle;
    if (_disposed) return;
    turn
      ..done = true
      ..stage = null
      ..toolLabel = null;
    if (reply.error case final error?) {
      turn
        ..error = error
        ..message = reply.message;
      _last = (body: request, timeout: timeout, turn: turn);
      _notify();
      return;
    }
    final result = reply.result!;
    String? text(String key) => switch (result[key]) {
      final String value when value.trim().isNotEmpty => value.trim(),
      _ => null,
    };
    turn
      ..question = text('transcript') ?? turn.question
      ..answer = text('answer') ?? turn.answer
      ..actions = [
        if (result['actions'] case final List<Object?> actions)
          for (final a in actions) ?AssistantAction.tryParse(a),
      ]
      ..pending = AssistantPending.tryParse(result['pending']);
    final conversation = text('conversation');
    turn.newConversation =
        continuing != null &&
        conversation != null &&
        conversation != continuing;
    _conversation = conversation;
    _conversationComputer = conversation == null ? null : computer.id;
    await _save();
    _notify();
  }

  void _onProgress(AssistantTurn turn, Map<String, Object?> progress) {
    final stage = progress['stage'];
    if (stage is! String) return;
    switch (stage) {
      case 'thinking':
        if (progress['transcript'] case final String transcript
            when transcript.trim().isNotEmpty) {
          turn.question = transcript.trim();
        }
        turn.toolLabel = null;
      case 'tool':
        turn.toolLabel = switch (progress['label']) {
          final String label when label.trim().isNotEmpty => label.trim(),
          _ => null,
        };
      case 'answer':
        // The whole answer so far: replaces, never appends.
        if (progress['text'] case final String text) turn.answer = text;
    }
    turn.stage = stage;
    _notify();
  }

  void _add(AssistantTurn turn) {
    _turns = [..._turns, turn];
    if (_turns.length > limit) _turns = _turns.sublist(_turns.length - limit);
  }

  Future<void> _save() async {
    final kept = [
      for (final turn in _turns)
        if (turn.done && turn.error == null) turn.toJson(),
    ];
    await _prefs.setString(
      prefsKey,
      jsonEncode({
        'conversation': ?_conversation,
        'conversationComputer': ?_conversationComputer,
        'turns': kept,
      }),
    );
  }

  void _load() {
    final raw = _prefs.getString(prefsKey);
    if (raw == null) return;
    try {
      if (jsonDecode(raw) case final Map<String, Object?> json) {
        _turns = [
          if (json['turns'] case final List<Object?> turns)
            for (final t in turns) ?AssistantTurn.fromJson(t),
        ];
        if (json case {
          'conversation': String conversation,
          'conversationComputer': String computer,
        }) {
          _conversation = conversation;
          _conversationComputer = computer;
        }
      }
    } on FormatException {
      // A damaged chat starts empty.
    }
  }

  /// At most [maxQuestionChars], counted in code points.
  static String _clip(String text) => text.runes.length <= maxQuestionChars
      ? text
      : String.fromCharCodes(text.runes.take(maxQuestionChars));

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _dictate.removeListener(_notify);
    _command?.removeListener(_notify);
    _voice.dispose();
    super.dispose();
  }
}
