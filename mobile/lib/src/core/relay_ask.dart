import 'dart:async';

import 'relay_client.dart';
import 'relay_protocol.dart';

/// A request's answer: the result when it is `ok`, else an error code (the
/// computer's, or the phone's own: offline, not_connected, interrupted,
/// timeout). A failed result's `message`, when it carries one, is
/// [message] (English, from the computer).
typedef RelayAnswer = ({
  Map<String, Object?>? result,
  String? error,
  String? message,
});

/// The wait while the computer asks its user to allow this phone; its own
/// prompt gives up after 2 minutes (PROTOCOL.md, Approval).
const approvalTimeout = Duration(minutes: 3);

/// Sends [body] to device [to] and waits for its result. The wait is
/// [timeout], started again by every `progress` (the computer is still at
/// it), and [approvalTimeout] while the last progress is `approval`.
/// [stageTimeouts] gives a stage its own wait: one after which the computer
/// may go quiet for long (a command at work, a tool the assistant runs). On
/// timeout the request is cancelled on the computer too.
Future<RelayAnswer> askComputer(
  RelayClient relay,
  String to,
  Map<String, Object?> body, {
  required Duration timeout,
  Map<String, Duration> stageTimeouts = const {},
  void Function(RelayRequest request)? onRequest,
  void Function(String stage)? onStage,
  void Function(Map<String, Object?> progress)? onProgress,
}) async {
  final request = relay.request(to, body);
  onRequest?.call(request);
  final result = Completer<Map<String, Object?>>();
  Timer? timer;
  void wait(Duration limit) {
    timer?.cancel();
    timer = Timer(limit, () {
      relay.cancel(request.id);
      if (!result.isCompleted) result.completeError(const _TimedOut());
    });
  }

  wait(timeout);
  final subscription = request.replies.listen(
    (reply) {
      if (reply['kind'] == RelayKind.progress) {
        if (reply['stage'] case final String stage) onStage?.call(stage);
        onProgress?.call(reply);
        final stage = reply['stage'];
        wait(
          stage == 'approval'
              ? approvalTimeout
              : stageTimeouts[stage] ?? timeout,
        );
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
      {'ok': true} && final reply => (
        result: reply,
        error: null,
        message: null,
      ),
      final reply => (
        result: null,
        error: reply['error'] is String ? reply['error']! as String : 'unknown',
        message: switch (reply['message']) {
          final String message when message.trim().isNotEmpty => message.trim(),
          _ => null,
        },
      ),
    };
  } on RelayException catch (e) {
    return (
      result: null,
      message: null,
      error: switch (e.error) {
        RelayError.offline => 'offline',
        RelayError.notConnected => 'not_connected',
        RelayError.interrupted => 'interrupted',
      },
    );
  } on _TimedOut {
    return (result: null, error: 'timeout', message: null);
  } finally {
    timer?.cancel();
    unawaited(subscription.cancel());
  }
}

class _TimedOut implements Exception {
  const _TimedOut();
}
