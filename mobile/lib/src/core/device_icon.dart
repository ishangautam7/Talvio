import 'package:flutter/material.dart';

import 'models.dart';

/// A device's platform icon with a dot while it is online.
class DeviceIcon extends StatelessWidget {
  const DeviceIcon(this.device, {super.key});

  final Device device;

  @override
  Widget build(BuildContext context) {
    return Badge(
      isLabelVisible: device.online,
      smallSize: 10,
      backgroundColor: Theme.of(context).colorScheme.primary,
      child: Icon(switch (device.platform) {
        'windows' => Icons.desktop_windows_outlined,
        'macos' => Icons.laptop_mac_outlined,
        'linux' => Icons.computer_outlined,
        'ios' => Icons.phone_iphone,
        'android' => Icons.phone_android,
        _ => Icons.devices_other_outlined,
      }),
    );
  }
}
