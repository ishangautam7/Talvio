import 'package:flutter/material.dart';

import 'remote_windows.dart';
import 'strings.dart';

/// One key of the pad, and how it is shown.
typedef _Key = ({String combo, String label, IconData? icon, String? text});

/// Keys to press on the computer, laid out as on a keyboard: the editing
/// keys on the left (Esc, Tab, Backspace; Space and a wide Enter under
/// them) and the arrows on the right as an inverted T, Up above Left, Down
/// and Right. Shift+Enter and the Ctrl (Cmd on a Mac) shortcuts are behind
/// "More keys", in the T's empty corner.
///
/// Used on the Send page and in live view, so the keys sit in the same place
/// in both.
class KeyPad extends StatelessWidget {
  const KeyPad({super.key, required this.platform, required this.onPress});

  /// The computer's, for Ctrl or Cmd.
  final String platform;
  final ValueChanged<List<String>>? onPress;

  static const double gap = 6;
  static const double clusterGap = 16;
  static const double keyHeight = 48;
  static const double _minKey = 44;
  static const double _maxKey = 64;

  /// The width six keys and their gaps take at the smallest key size.
  static const double minWidth = 6 * _minKey + 4 * gap + clusterGap;

  /// The pad's height.
  static const double height = 2 * keyHeight + gap;

  List<(String, String)> _more() {
    final mod = DictateStrings.modifier(platform);
    return [
      ('shift+enter', DictateStrings.keyShiftEnter),
      (commandCombo(platform, 'c'), DictateStrings.keyCopy(mod)),
      (commandCombo(platform, 'v'), DictateStrings.keyPaste(mod)),
      (commandCombo(platform, 'z'), DictateStrings.keyUndo(mod)),
      (commandCombo(platform, 'a'), DictateStrings.keySelectAll(mod)),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final onPress = this.onPress;
    return LayoutBuilder(
      builder: (context, constraints) {
        final room = constraints.hasBoundedWidth
            ? constraints.maxWidth
            : minWidth;
        final w = ((room - 4 * gap - clusterGap) / 6).clamp(_minKey, _maxKey);
        Widget key(_Key k, {int span = 1, bool primary = false}) => _PadKey(
          k,
          width: w * span + gap * (span - 1),
          primary: primary,
          onPressed: onPress == null ? null : () => onPress([k.combo]),
        );
        final blank = SizedBox(width: w);
        final more = SizedBox(
          width: w,
          height: keyHeight,
          child: PopupMenuButton<String>(
            tooltip: DictateStrings.moreKeys,
            enabled: onPress != null,
            icon: const Icon(Icons.more_horiz),
            onSelected: (combo) => onPress?.call([combo]),
            itemBuilder: (context) => [
              for (final (combo, label) in _more())
                PopupMenuItem(value: combo, child: Text(label)),
            ],
          ),
        );
        Widget row(List<Widget> left, List<Widget> right) => Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ..._spaced(left),
            const SizedBox(width: clusterGap),
            ..._spaced(right),
          ],
        );
        // Wider than the room only on the very narrowest phones.
        return FittedBox(
          fit: BoxFit.scaleDown,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              row(
                [key(_escape), key(_tab), key(_backspace)],
                [blank, key(_up), more],
              ),
              const SizedBox(height: gap),
              row(
                [key(_space), key(_enter, span: 2, primary: true)],
                [key(_left), key(_down), key(_right)],
              ),
            ],
          ),
        );
      },
    );
  }

  static List<Widget> _spaced(List<Widget> keys) => [
    for (final (i, key) in keys.indexed) ...[
      if (i > 0) const SizedBox(width: gap),
      key,
    ],
  ];
}

const _Key _escape = (
  combo: 'escape',
  label: DictateStrings.keyEscapeLabel,
  icon: null,
  text: DictateStrings.keyEscape,
);
const _Key _tab = (
  combo: 'tab',
  label: DictateStrings.keyTab,
  icon: Icons.keyboard_tab,
  text: null,
);
const _Key _backspace = (
  combo: 'backspace',
  label: DictateStrings.keyBackspace,
  icon: Icons.backspace_outlined,
  text: null,
);
const _Key _space = (
  combo: 'space',
  label: DictateStrings.keySpace,
  icon: Icons.space_bar,
  text: null,
);
const _Key _enter = (
  combo: 'enter',
  label: DictateStrings.keyEnter,
  icon: Icons.keyboard_return,
  text: DictateStrings.keyEnter,
);
const _Key _up = (
  combo: 'up',
  label: DictateStrings.keyUp,
  icon: Icons.keyboard_arrow_up,
  text: null,
);
const _Key _left = (
  combo: 'left',
  label: DictateStrings.keyLeft,
  icon: Icons.keyboard_arrow_left,
  text: null,
);
const _Key _down = (
  combo: 'down',
  label: DictateStrings.keyDown,
  icon: Icons.keyboard_arrow_down,
  text: null,
);
const _Key _right = (
  combo: 'right',
  label: DictateStrings.keyRight,
  icon: Icons.keyboard_arrow_right,
  text: null,
);

class _PadKey extends StatelessWidget {
  const _PadKey(
    this.k, {
    required this.width,
    required this.primary,
    required this.onPressed,
  });

  final _Key k;
  final double width;
  final bool primary;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final shape = RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(12),
    );
    final size = Size(width, KeyPad.keyHeight);
    final content = FittedBox(
      fit: BoxFit.scaleDown,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (k.icon case final icon?) Icon(icon, size: 22),
          if (k.icon != null && k.text != null) const SizedBox(width: 6),
          if (k.text case final text?) Text(text),
        ],
      ),
    );
    final button = primary
        ? FilledButton.tonal(
            onPressed: onPressed,
            style: FilledButton.styleFrom(
              fixedSize: size,
              minimumSize: size,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              shape: shape,
            ),
            child: content,
          )
        : OutlinedButton(
            onPressed: onPressed,
            style: OutlinedButton.styleFrom(
              fixedSize: size,
              minimumSize: size,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              shape: shape,
            ),
            child: content,
          );
    return Semantics(
      label: k.label,
      button: true,
      enabled: onPressed != null,
      onTap: onPressed,
      // Excluding the button's own semantics drops its tap too.
      excludeSemantics: true,
      child: Tooltip(message: k.label, child: button),
    );
  }
}
