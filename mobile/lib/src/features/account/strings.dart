abstract final class AccountStrings {
  static const title = 'Account';
  static const devices = 'Devices';
  static const connecting = 'Connecting to the server…';
  static const thisPhone = 'This phone';
  static const serverNote = 'Found automatically. Talvio keeps it up to date.';
  static const licences = 'Licences';
  static String version(String version) => 'Version $version';
  static const online = 'Online';
  static const server = 'Server';
  static const developmentServer = 'Development server';
  static const signOut = 'Sign out';
  static const deleteAccount = 'Delete account';
  static const cancel = 'Cancel';
  static const remove = 'Remove';
  static String removeTooltip(String name) => 'Remove $name';
  static String removeTitle(String name) => 'Remove $name?';
  static const removeBody =
      'It will be signed out of Talvio. To use it with this phone again, '
      'sign in on it.';
  static const sendFeedback = 'Send feedback';
  static const deleteTitle = 'Delete your account?';
  static const deleteBody =
      'This permanently deletes your Talvio account and signs out every '
      "device, including this phone. It can't be undone.";

  static String lastSeen(DateTime at, DateTime now) =>
      'Last seen ${ago(at, now)}';

  /// How long ago [at] was: "just now", "5 min ago", "yesterday",
  /// "3 months ago", "over a year ago".
  static String ago(DateTime at, DateTime now) {
    final ago = now.difference(at);
    if (ago.inMinutes < 1) return 'just now';
    if (ago.inHours < 1) return '${ago.inMinutes} min ago';
    if (ago.inDays < 1) return '${ago.inHours} h ago';
    if (ago.inDays == 1) return 'yesterday';
    if (ago.inDays < 30) return '${ago.inDays} days ago';
    if (ago.inDays < 365) {
      final months = ago.inDays ~/ 30;
      return months <= 1 ? '1 month ago' : '$months months ago';
    }
    final years = ago.inDays ~/ 365;
    return years <= 1 ? 'over a year ago' : '$years years ago';
  }
}

abstract final class FeedbackStrings {
  static const title = 'Send feedback';
  static const intro =
      'A bug, an idea or a question: say it however it comes out. '
      'It goes straight to the person who makes Talvio.';
  static const message = 'Message';
  static const messageHint = 'What happened, or what would make Talvio better?';
  static const messageShort =
      "Write a few words so there's something to go on.";
  static const messageLong = "That's longer than 5,000 characters.";
  static const email = "Email, if you'd like a reply";
  static const emailInvalid = "That email address doesn't look right.";
  static const includeSystem = 'Include app version and phone';
  static String sentAs(String line) => 'Sent exactly as: $line';
  static const send = 'Send';
  static const sentTitle = "Thanks, it's on its way";
  static const sentBody = "It's with the maintainer now.";
  static String sentWithEmail(String email) =>
      "If there's something to follow up on, you'll hear back at $email.";
  static const done = 'Done';
  static const rejected =
      "The feedback couldn't be accepted. Try shortening it and send again.";
  static const unreachable =
      "Can't reach the feedback server. Check your connection; your message "
      'is kept here.';
  static const serverTrouble =
      "The feedback server isn't answering properly. Your message is kept "
      'here, so try again later.';
}
