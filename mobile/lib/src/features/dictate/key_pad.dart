import 'package:flutter/material.dart';

import '../../design/components.dart';
import '../../design/tokens.dart';
import 'remote_windows.dart';
import 'strings.dart';

/// One key of the pad, and how it is shown.
typedef _Key = ({String combo, String label, IconData? icon, String? text});

/// Keys to press on the computer, laid out as on a keyboard (design spec
/// §3.13): the editing keys on the left (Esc, Tab, Backspace; a wide Space
/// and Enter under them) and the arrows on the right as an inverted T, Up
/// above Left, Down and Right. Shift+Enter and the Ctrl (Cmd on a Mac)
/// shortcuts are behind "More keys", in the T's empty corner.
///
/// Used on the Dictate page and in live view, so the keys sit in the same
/// place in both.
class KeyPad extends StatelessWidget {
  const KeyPad({super.key, required this.platform, required this.onPress});

  /// The computer's, for Ctrl or Cmd.
  final String platform;
  final ValueChanged<List<String>>? onPress;

  static const double gap = 8;
  static const double clusterGap = 16;
  static const double keyHeight = 48;
  static const double _minKey = 48;
  static const double _maxKey = 72;

  /// The width six keys and their gaps take at the smallest key size.
  static const double minWidth = 6 * _minKey + 4 * gap + clusterGap;

  /// The pad's height.
  static const double height = 2 * keyHeight + gap;

  List<(String combo, String title, String keys, String label)> _more() {
    final mod = DictateStrings.modifier(platform);
    return [
      (
        'shift+enter',
        DictateStrings.newLine,
        DictateStrings.shiftEnter,
        DictateStrings.keyShiftEnter,
      ),
      (
        commandCombo(platform, 'c'),
        DictateStrings.copy,
        DictateStrings.combo(mod, 'C'),
        DictateStrings.keyCopy(mod),
      ),
      (
        commandCombo(platform, 'v'),
        DictateStrings.paste,
        DictateStrings.combo(mod, 'V'),
        DictateStrings.keyPaste(mod),
      ),
      (
        commandCombo(platform, 'z'),
        DictateStrings.undo,
        DictateStrings.combo(mod, 'Z'),
        DictateStrings.keyUndo(mod),
      ),
      (
        commandCombo(platform, 'a'),
        DictateStrings.selectAll,
        DictateStrings.combo(mod, 'A'),
        DictateStrings.keySelectAll(mod),
      ),
    ];
  }

  Future<void> _openMore(BuildContext context) => showTalvioSheet<void>(
    context,
    title: DictateStrings.moreKeys,
    builder: (sheet) {
      final t = sheet.talvio;
      return SingleChildScrollView(
        child: SheetRows(
          dividerInset: 16,
          children: [
            for (final (combo, title, keys, label) in _more())
              ListRow(
                title: title,
                semanticLabel: label,
                trailing: Text(
                  keys,
                  style: TalvioType.mono.copyWith(color: t.muted),
                ),
                onTap: () {
                  Navigator.pop(sheet);
                  onPress?.call([combo]);
                },
              ),
          ],
        ),
      );
    },
  );

  @override
  Widget build(BuildContext context) {
    final onPress = this.onPress;
    return LayoutBuilder(
      builder: (context, constraints) {
        final room = constraints.hasBoundedWidth
            ? constraints.maxWidth
            : minWidth;
        final w = ((room - 4 * gap - clusterGap) / 6).clamp(_minKey, _maxKey);
        Widget key(_Key k, {int span = 1, bool primary = false}) => KeyCap(
          label: k.label,
          icon: k.icon,
          text: k.text,
          width: w * span + gap * (span - 1),
          primary: primary,
          onPressed: onPress == null ? null : () => onPress([k.combo]),
        );
        final blank = SizedBox(width: w);
        final more = KeyCap(
          label: DictateStrings.moreKeys,
          icon: AppIcons.more,
          width: w,
          onPressed: onPress == null ? null : () => _openMore(context),
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
                [key(_space, span: 2), key(_enter, primary: true)],
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
  icon: AppIcons.tab,
  text: null,
);
const _Key _backspace = (
  combo: 'backspace',
  label: DictateStrings.keyBackspace,
  icon: AppIcons.backspace,
  text: null,
);
const _Key _space = (
  combo: 'space',
  label: DictateStrings.keySpace,
  icon: null,
  text: DictateStrings.keySpace,
);
const _Key _enter = (
  combo: 'enter',
  label: DictateStrings.keyEnter,
  icon: AppIcons.enter,
  text: null,
);
const _Key _up = (
  combo: 'up',
  label: DictateStrings.keyUp,
  icon: AppIcons.caretUp,
  text: null,
);
const _Key _left = (
  combo: 'left',
  label: DictateStrings.keyLeft,
  icon: AppIcons.caretLeft,
  text: null,
);
const _Key _down = (
  combo: 'down',
  label: DictateStrings.keyDown,
  icon: AppIcons.caretDown,
  text: null,
);
const _Key _right = (
  combo: 'right',
  label: DictateStrings.keyRight,
  icon: AppIcons.caretRight,
  text: null,
);

/// One key: 48 tall, radius 12, a keycap's bottom edge on light; [primary]
/// is the ink Enter.
class KeyCap extends StatefulWidget {
  const KeyCap({
    super.key,
    required this.label,
    required this.width,
    required this.onPressed,
    this.icon,
    this.text,
    this.primary = false,
  });

  final String label;
  final double width;
  final VoidCallback? onPressed;
  final IconData? icon;
  final String? text;
  final bool primary;

  @override
  State<KeyCap> createState() => _KeyCapState();
}

class _KeyCapState extends State<KeyCap> {
  var _down = false;

  void _set(bool down) {
    if (widget.onPressed != null && down != _down) {
      setState(() => _down = down);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final enabled = widget.onPressed != null;
    final fg = widget.primary ? t.canvas : t.ink;
    final bg = widget.primary
        ? t.ink
        : _down
        ? t.fillStrong
        : t.isDark
        ? t.fill
        : t.card;
    final content = FittedBox(
      fit: BoxFit.scaleDown,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (widget.icon case final icon?) Icon(icon, size: 20, color: fg),
          if (widget.text case final text?)
            Text(
              text,
              style: TalvioType.label.copyWith(
                color: fg,
                fontWeight: FontWeight.w600,
              ),
            ),
        ],
      ),
    );
    return Semantics(
      label: widget.label,
      button: true,
      enabled: enabled,
      onTap: widget.onPressed,
      excludeSemantics: true,
      child: Tooltip(
        message: widget.label,
        excludeFromSemantics: true,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapDown: (_) => _set(true),
          onTapUp: (_) => _set(false),
          onTapCancel: () => _set(false),
          onTap: widget.onPressed,
          child: AnimatedScale(
            scale: _down && motionAllowed(context) ? 0.95 : 1,
            duration: Motion.press,
            child: Opacity(
              opacity: enabled ? 1 : 0.38,
              child: Container(
                width: widget.width,
                height: KeyPad.keyHeight,
                padding: const EdgeInsets.symmetric(horizontal: 4),
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: widget.primary && _down
                      ? Color.alphaBlend(t.pressedOnAccent, bg)
                      : bg,
                  borderRadius: BorderRadius.circular(Radii.md),
                  boxShadow: t.isDark || widget.primary
                      ? null
                      : [
                          BoxShadow(
                            color: t.border,
                            offset: const Offset(0, 1),
                          ),
                        ],
                ),
                child: content,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
