import '../../core/api_client.dart';
import '../../core/strings.dart';
import 'strings.dart';

/// The rules of the feedback form, apart from its screen (PROTOCOL.md,
/// "Feedback"; the same limits as the server and the desktop app).
abstract final class FeedbackRules {
  static const minChars = 3;
  static const maxChars = 5000;

  /// Characters as the server counts them (code points), after trimming.
  static int length(String message) => message.trim().runes.length;

  static bool messageOk(String message) {
    final chars = length(message);
    return chars >= minChars && chars <= maxChars;
  }

  /// Loose on purpose: catch a typo, not police RFC 5322. The desktop app's
  /// rule.
  static bool looksLikeEmail(String email) {
    if (email.length > 254 || RegExp(r'\s').hasMatch(email)) return false;
    final parts = email.split('@');
    if (parts.length != 2) return false;
    final [local, domain] = parts;
    return local.isNotEmpty &&
        domain.contains('.') &&
        !domain.startsWith('.') &&
        !domain.endsWith('.') &&
        !domain.contains('..');
  }

  /// Blank is fine (no reply wanted); anything else must look like an address.
  static bool emailOk(String email) =>
      email.trim().isEmpty || looksLikeEmail(email.trim());

  /// The line shown under "Include app version and phone" and sent verbatim
  /// when it is ticked.
  static String systemLine({
    required String appVersion,
    required String os,
    required String osVersion,
  }) {
    final name = switch (os) {
      'android' => 'Android',
      'ios' => 'iOS',
      _ => os,
    };
    final version = osVersion.trim();
    final line = version.isEmpty
        ? 'Talvio phone $appVersion · $name'
        : 'Talvio phone $appVersion · $name $version';
    // The server keeps at most 300 characters of it.
    final runes = line.runes.toList();
    return runes.length <= 300 ? line : String.fromCharCodes(runes.take(300));
  }

  /// What a failed send says.
  static String errorText(ApiException error) => switch (error.code) {
    'invalid_email' => FeedbackStrings.emailInvalid,
    'invalid_request' => FeedbackStrings.rejected,
    'payload_too_large' => FeedbackStrings.rejected,
    'network' => FeedbackStrings.unreachable,
    'internal' => FeedbackStrings.serverTrouble,
    _ => commonErrorText(error),
  };
}
