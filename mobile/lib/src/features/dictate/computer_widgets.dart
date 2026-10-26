// The computer this phone talks to, as every page shows it: the switcher at
// the top of the app, the sheet that changes it, and the states before
// there is one to talk to.

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import '../account/strings.dart';
import 'dictate_controller.dart';
import 'remote_windows.dart';
import 'strings.dart';

export '../../design/icons.dart' show platformIcon;

/// A window as two lines: the app, and its title (null when the app is all
/// there is to say).
(String, String?) windowLabel(RemoteWindow window) => switch (window) {
  RemoteWindow(app: '', title: '') => (DictateStrings.untitled, null),
  RemoteWindow(app: '', :final title) => (title, null),
  RemoteWindow(:final app, title: '') => (app, null),
  RemoteWindow(:final app, :final title) => (app, title),
};

/// "Online · Windows", or "Offline".
String presence(Device computer) => computer.online
    ? DictateStrings.onlineOn(computer.platform)
    : DictateStrings.offline;

/// "Online · Windows", or "Offline · last seen 3 days ago".
String presenceLong(Device computer) => computer.online
    ? DictateStrings.onlineOn(computer.platform)
    : '${DictateStrings.offline} · '
          '${AccountStrings.lastSeen(computer.lastSeenAt, DateTime.now()).toLowerCase()}';

/// Whether the user may switch computers now: there is more than one, and
/// no recording or request is holding on to this one.
bool canChooseComputer(DictateController controller) =>
    controller.computers.length > 1 && controller.phase == DictatePhase.idle;

/// The sheet that switches the computer to talk to.
Future<void> chooseComputer(
  BuildContext context,
  DictateController controller,
) => showTalvioSheet<void>(
  context,
  title: DictateStrings.pickComputerTitle,
  builder: (sheet) => SingleChildScrollView(
    child: SheetRows(
      children: [
        for (final computer in controller.computers)
          ComputerRow(
            computer,
            selected: computer.id == controller.computer?.id,
            onTap: () {
              Haptic.tick();
              controller.choose(computer.id);
              Navigator.pop(sheet);
            },
          ),
      ],
    ),
  ),
);

/// The top of the app: the computer's name and how it is, one button that
/// opens the switcher when there are several. [connecting] says the server
/// link is down.
class ComputerSwitcher extends StatefulWidget {
  const ComputerSwitcher({
    super.key,
    required this.computer,
    required this.onTap,
    this.connecting = false,
  });

  final Device computer;
  final VoidCallback? onTap;
  final bool connecting;

  @override
  State<ComputerSwitcher> createState() => _ComputerSwitcherState();
}

class _ComputerSwitcherState extends State<ComputerSwitcher> {
  var _down = false;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    final computer = widget.computer;
    final onTap = widget.onTap;
    final status = widget.connecting
        ? DictateStrings.connectingShort
        : presence(computer);
    final dot = widget.connecting
        ? StatusDot(color: t.warning, pulse: true)
        : StatusDot(color: computer.online ? t.success : t.faint);
    return Semantics(
      button: onTap != null,
      label: DictateStrings.chooseComputer,
      value: '${computer.name}, $status',
      excludeSemantics: true,
      onTap: onTap,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: onTap == null ? null : (_) => setState(() => _down = true),
        onTapUp: onTap == null ? null : (_) => setState(() => _down = false),
        onTapCancel: onTap == null ? null : () => setState(() => _down = false),
        onTap: onTap == null
            ? null
            : () {
                Haptic.tick();
                onTap();
              },
        child: Padding(
          // A wider hit area, with the text still at the gutter.
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 44),
            child: AnimatedOpacity(
              duration: Motion.press,
              opacity: _down ? 0.6 : 1,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Flexible(
                        child: Text(
                          computer.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TalvioType.title2.copyWith(color: t.ink),
                        ),
                      ),
                      if (onTap != null) ...[
                        const SizedBox(width: Space.s4),
                        Icon(AppIcons.caretDown, size: 16, color: t.muted),
                      ],
                    ],
                  ),
                  const SizedBox(height: Space.s2),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      SizedBox.square(
                        dimension: 6,
                        child: OverflowBox(
                          maxWidth: 14,
                          maxHeight: 14,
                          child: Center(child: dot),
                        ),
                      ),
                      const SizedBox(width: 6),
                      Flexible(
                        child: AnimatedSwitcher(
                          duration: Motion.quick,
                          child: Text(
                            status,
                            key: ValueKey(status),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TalvioType.footnote.copyWith(color: t.muted),
                          ),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// A computer as a row: its platform on a tile with its presence dot.
class ComputerRow extends StatelessWidget {
  const ComputerRow(
    this.computer, {
    super.key,
    required this.selected,
    required this.onTap,
  });

  final Device computer;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return ListRow(
      leading: RowTile(
        platformIcon(computer.platform),
        presence: computer.online ? t.success : t.faint,
      ),
      title: computer.name,
      subtitle: presenceLong(computer),
      trailing: selected
          ? Icon(AppIcons.check, size: 20, color: t.accent)
          : null,
      selected: selected,
      onTap: onTap,
    );
  }
}

/// Several computers and no choice yet (or the chosen one left the account).
class PickComputer extends StatelessWidget {
  const PickComputer(this.computers, {super.key, required this.onPick});

  final List<Device> computers;
  final ValueChanged<String> onPick;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return ListView(
      padding: const EdgeInsets.only(top: Space.s24, bottom: Space.s24),
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 32),
          child: Text(
            DictateStrings.pickComputerTitle,
            style: TalvioType.title1.copyWith(color: t.ink),
          ),
        ),
        const SizedBox(height: Space.s8),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 32),
          child: Text(
            DictateStrings.pickComputerBody,
            style: TalvioType.rowBody.copyWith(color: t.muted),
          ),
        ),
        const SizedBox(height: Space.s24),
        Group(
          children: [
            for (final computer in computers)
              ComputerRow(
                computer,
                selected: false,
                onTap: () {
                  Haptic.tick();
                  onPick(computer.id);
                },
              ),
          ],
        ),
      ],
    );
  }
}

class NoComputer extends StatelessWidget {
  const NoComputer({super.key, required this.email});

  final String email;

  @override
  Widget build(BuildContext context) => EmptyState(
    icon: DuoIcons.addComputer,
    title: DictateStrings.noComputerTitle,
    body: DictateStrings.noComputerBody(email),
  );
}

/// The body every page shows until there is a computer: a spinner before
/// the device list arrives, the chooser when the user has to pick, how to
/// add one when there is none; else [builder] with the computer.
class ComputerGate extends StatelessWidget {
  const ComputerGate({
    super.key,
    required this.controller,
    required this.builder,
  });

  final DictateController controller;
  final Widget Function(BuildContext context, Device computer, bool connected)
  builder;

  @override
  Widget build(BuildContext context) {
    final services = AppServices.of(context);
    final relay = services.relay;
    final connected = relay.status.value == RelayStatus.connected;
    final known = relay.devices.value != null;
    final computers = controller.computers;
    return switch ((known, controller.computer)) {
      (false, _) => const EmptyState(
        icon: DuoIcons.addComputer,
        title: DictateStrings.connecting,
        busy: true,
      ),
      (true, null) when computers.isNotEmpty => PickComputer(
        computers,
        onPick: controller.choose,
      ),
      (true, null) => NoComputer(
        email: services.session.session?.user.email ?? '',
      ),
      (true, final Device computer) => builder(context, computer, connected),
    };
  }
}
