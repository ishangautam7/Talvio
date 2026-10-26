// Choosing which window on the computer receives phone input, and pressing
// keys there (PROTOCOL.md, "Windows: choosing where phone input goes").

/// The capability and request kinds of this part of the feature.
abstract final class WindowsKind {
  static const capability = 'windows';
  static const list = 'windows.list';
  static const keys = 'windows.keys';
}

/// One window a `windows.list` offered. [id] is valid for this phone until
/// its next `windows.list` or 30 minutes, whichever is first.
class RemoteWindow {
  const RemoteWindow({
    required this.id,
    required this.title,
    required this.app,
    required this.front,
  });

  /// Null for an entry the protocol does not allow, so one bad entry drops
  /// itself rather than the whole list.
  static RemoteWindow? tryParse(Object? json) => switch (json) {
    {'id': String id, 'title': String title, 'app': String app}
        when id.isNotEmpty && id.length <= 64 =>
      RemoteWindow(
        id: id,
        title: title.trim(),
        app: app.trim(),
        front: json['front'] == true,
      ),
    _ => null,
  };

  final String id;
  final String title;
  final String app;

  /// The window in front on the computer.
  final bool front;

  /// Two windows are taken for the same one when app and title agree.
  bool sameAs(RemoteWindow other) => app == other.app && title == other.title;
}

/// The windows of a `windows.list` result; empty when it lists none.
List<RemoteWindow> windowsOf(Map<String, Object?> result) => switch (result) {
  {'windows': List<Object?> windows} => [
    for (final json in windows) ?RemoteWindow.tryParse(json),
  ],
  _ => const [],
};

/// The combo for a shortcut that is Ctrl on Windows and Linux and Cmd on a
/// Mac ([platform] as in `Device.platform`).
String commandCombo(String platform, String key) =>
    '${platform == 'macos' ? 'cmd' : 'ctrl'}+$key';
