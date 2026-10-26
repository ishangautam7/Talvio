import 'package:flutter_test/flutter_test.dart';
import 'package:talvio_mobile/src/features/dictate/dictate_screen.dart';
import 'package:talvio_mobile/src/features/dictate/remote_windows.dart';

void main() {
  group('windowsOf', () {
    test('reads a windows.list result, front first as sent', () {
      final windows = windowsOf({
        'kind': 'result',
        'requestId': 'r',
        'ok': true,
        'windows': [
          {
            'id': 'w1',
            'title': 'Claude Code — zynveliq',
            'app': 'Windows Terminal',
            'front': true,
          },
          {'id': 'w2', 'title': ' notes.txt ', 'app': 'Notepad'},
        ],
      });
      expect(windows.map((w) => w.id), ['w1', 'w2']);
      expect(windows.first.front, isTrue);
      expect(windows.first.app, 'Windows Terminal');
      expect(windows.last.front, isFalse, reason: 'absent means not front');
      expect(windows.last.title, 'notes.txt', reason: 'trimmed');
    });

    test('drops entries the protocol does not allow, keeps the rest', () {
      final windows = windowsOf({
        'windows': [
          {'id': 'ok', 'title': 'A', 'app': 'B', 'front': false},
          {'id': '', 'title': 'A', 'app': 'B'},
          {'id': 'x' * 65, 'title': 'A', 'app': 'B'},
          {'id': 7, 'title': 'A', 'app': 'B'},
          {'id': 'no-title', 'app': 'B'},
          'w3',
          null,
        ],
      });
      expect(windows.map((w) => w.id), ['ok']);
    });

    test('a result without a list lists nothing', () {
      expect(windowsOf({'ok': true}), isEmpty);
      expect(windowsOf({'windows': 'w1'}), isEmpty);
    });
  });

  test('a window reads as app and title, whichever it has', () {
    RemoteWindow window(String app, String title) =>
        RemoteWindow(id: 'w', title: title, app: app, front: false);
    expect(windowLabel(window('Notepad', 'notes.txt')), (
      'Notepad',
      'notes.txt',
    ));
    expect(windowLabel(window('Notepad', '')), ('Notepad', null));
    expect(windowLabel(window('', 'notes.txt')), ('notes.txt', null));
    expect(windowLabel(window('', '')), ('Untitled', null));
  });

  test('copy and paste are Cmd on a Mac and Ctrl elsewhere', () {
    expect(commandCombo('macos', 'c'), 'cmd+c');
    expect(commandCombo('windows', 'v'), 'ctrl+v');
    expect(commandCombo('linux', 'z'), 'ctrl+z');
  });
}
