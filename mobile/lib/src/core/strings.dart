import 'api_client.dart';
import 'session_store.dart';

/// The account server as screens show it: how it was chosen, then its host.
String serverText(ServerChoice choice, String url) {
  final host = Uri.parse(url).authority;
  return switch (choice) {
    ServerChoice.automatic => 'Automatic · $host',
    ServerChoice.custom => 'Custom · $host',
    ServerChoice.pinned => 'Server: $host',
  };
}

/// Text for the API errors any screen can hit. Features add their own codes
/// in front of this.
String commonErrorText(ApiException error) => switch (error.code) {
  'network' => "Can't reach the server. Check your connection and try again.",
  'rate_limited' => _rateLimited(error.retryAfter),
  'unauthorized' => 'You were signed out. Sign in again.',
  _ => 'Something went wrong. Try again.',
};

String _rateLimited(Duration? wait) {
  if (wait == null || wait.inSeconds <= 0) {
    return 'Too many attempts. Wait a moment and try again.';
  }
  if (wait.inSeconds < 90) {
    return 'Too many attempts. Try again in ${wait.inSeconds} seconds.';
  }
  final minutes = (wait.inSeconds / 60).ceil();
  return 'Too many attempts. Try again in $minutes minutes.';
}
