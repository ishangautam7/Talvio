// The computer this phone talks to, as every page shows it: the switcher at
// the top of the app, the sheet that changes it, and the states before
// there is one to talk to.

import 'package:flutter/material.dart';

import '../../core/app_services.dart';
import '../../core/models.dart';
import '../../core/relay_client.dart';
import '../../design/components.dart';
import '../../design/tokens.dart';
import 'dictate_controller.dart';
import 'remote_windows.dart';
import 'strings.dart';

TextStyle? mutedText(BuildContext context) =>
    context.text.bodyMedium?.copyWith(color: context.talvio.muted);

/// A window as two lines: the app, and its title (null when the app is all
/// there is to say).
(String, String?) windowLabel(RemoteWindow window) => switch (window) {
  RemoteWindow(app: '', title: '') => (DictateStrings.untitled, null),
  RemoteWindow(app: '', :final title) => (title, null),
  RemoteWindow(:final app, title: '') => (app, null),
  RemoteWindow(:final app, :final title) => (app, title),
};

/// The platform's icon.
IconData platformIcon(String platform) => switch (platform) {
  'windows' => Icons.desktop_windows_rounded,
  'macos' => Icons.laptop_mac_rounded,
  'linux' => Icons.computer_rounded,
  'ios' => Icons.phone_iphone_rounded,
  'android' => Icons.phone_android_rounded,
  _ => Icons.devices_other_rounded,
};

/// "Online · Windows", or "Offline".
String presence(Device computer) => computer.online
    ? DictateStrings.onlineOn(computer.platform)
    : DictateStrings.offline;

/// Whether the user may switch computers now: there is more than one, and
/// no recording or request is holding on to this one.
bool canChooseComputer(DictateController controller) =>
    controller.computers.length > 1 && controller.phase == DictatePhase.idle;

/// The sheet that switches the computer to talk to.
Future<void> chooseComputer(
  BuildContext context,
  DictateController controller,
) => showModalBottomSheet<void>(
  context: context,
  builder: (context) => SafeArea(
    child: ListView(
      shrinkWrap: true,
      padding: const EdgeInsets.only(bottom: Space.lg),
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
            Space.page,
            0,
            Space.page,
            Space.md,
          ),
          child: Text(
            DictateStrings.chooseComputer,
            style: context.text.titleLarge,
          ),
        ),
        for (final computer in controller.computers)
          ComputerTile(
            computer,
            selected: computer.id == controller.computer?.id,
            onTap: () {
              Haptic.tick();
              controller.choose(computer.id);
              Navigator.pop(context);
            },
          ),
      ],
    ),
  ),
);

/// The computer's icon on the ink tile, with its presence dot.
class ComputerAvatar extends StatelessWidget {
  const ComputerAvatar(this.computer, {super.key, this.size = 40});

  final Device computer;
  final double size;

  @override
  Widget build(BuildContext context) {
    final t = context.talvio;
    return SizedBox.square(
      dimension: size + 4,
      child: Stack(
        children: [
          Container(
            width: size,
            height: size,
            decoration: BoxDecoration(
              gradient: TalvioColors.inkTile,
              borderRadius: BorderRadius.circular(size * 0.3),
            ),
            child: Icon(
              platformIcon(computer.platform),
              size: size * 0.5,
              color: Colors.white,
            ),
          ),
          Positioned(
            right: 0,
            bottom: 0,
            child: Container(
              width: size * 0.36,
              height: size * 0.36,
              decoration: BoxDecoration(
                color: computer.online ? t.success : t.faint,
                shape: BoxShape.circle,
                border: Border.all(color: t.canvas, width: 2.5),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The top of the app: the computer, its presence, and the switcher when
/// there are several. [connecting] says the server link is down.
class ComputerSwitcher extends StatelessWidget {
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
  Widget build(BuildContext context) {
    final t = context.talvio;
    final status = connecting
        ? DictateStrings.connectingShort
        : presence(computer);
    return Semantics(
      button: onTap != null,
      label: DictateStrings.chooseComputer,
      value: '${computer.name}, $status',
      excludeSemantics: true,
      onTap: onTap,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(Radii.md),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4, horizontal: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              ComputerAvatar(computer, size: 36),
              const SizedBox(width: Space.sm),
              Flexible(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Flexible(
                          child: Text(
                            computer.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: context.text.titleMedium?.copyWith(
                              fontWeight: FontWeight.w700,
                            ),
                          ),
                        ),
                        if (onTap != null)
                          Icon(
                            Icons.expand_more_rounded,
                            size: 20,
                            color: t.muted,
                          ),
                      ],
                    ),
                    Text(
                      status,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: context.text.bodySmall?.copyWith(
                        color: connecting
                            ? t.warning
                            : computer.online
                            ? t.success
                            : t.muted,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class ComputerTile extends StatelessWidget {
  const ComputerTile(
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
    return ListTile(
      minVerticalPadding: 10,
      leading: ComputerAvatar(computer, size: 40),
      title: Text(computer.name, style: context.text.titleMedium),
      subtitle: Text(presence(computer)),
      trailing: selected
          ? Icon(Icons.check_circle_rounded, color: context.talvio.accent)
          : null,
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
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: Space.xxl),
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
            Space.page,
            0,
            Space.page,
            Space.xs,
          ),
          child: Text(
            DictateStrings.pickComputerTitle,
            style: context.text.headlineMedium,
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(
            Space.page,
            0,
            Space.page,
            Space.lg,
          ),
          child: Text(
            DictateStrings.pickComputerBody,
            style: mutedText(context),
          ),
        ),
        for (final computer in computers)
          ComputerTile(
            computer,
            selected: false,
            onTap: () {
              Haptic.tick();
              onPick(computer.id);
            },
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
    icon: Icons.add_to_queue_rounded,
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
      (false, _) => EmptyState(
        icon: Icons.wifi_tethering_rounded,
        title: DictateStrings.connectingTitle,
        body: DictateStrings.connecting,
        tone: Tone.neutral,
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
