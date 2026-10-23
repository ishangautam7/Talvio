abstract final class AccountStrings {
  static const tab = 'Account';
  static const title = 'Account';
  static const devices = 'Devices';
  static const connecting = 'Connecting to the server…';
  static const thisPhone = 'This phone';
  static const online = 'Online';
  static const server = 'Server';
  static const signOut = 'Sign out';
  static const deleteAccount = 'Delete account';
  static const cancel = 'Cancel';
  static const remove = 'Remove';
  static String removeTooltip(String name) => 'Remove $name';
  static String removeTitle(String name) => 'Remove $name?';
  static const removeBody =
      'It will be signed out of Talvio. To use it with this phone again, '
      'sign in on it.';
  static const deleteTitle = 'Delete your account?';
  static const deleteBody =
      'This permanently deletes your Talvio account and signs out every '
      "device, including this phone. It can't be undone.";

  static String lastSeen(DateTime at, DateTime now) {
    final ago = now.difference(at);
    if (ago.inMinutes < 1) return 'Last seen just now';
    if (ago.inHours < 1) return 'Last seen ${ago.inMinutes} min ago';
    if (ago.inDays < 1) return 'Last seen ${ago.inHours} h ago';
    if (ago.inDays == 1) return 'Last seen yesterday';
    return 'Last seen ${ago.inDays} days ago';
  }
}
