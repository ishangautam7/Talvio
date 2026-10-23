// The objects of server/PROTOCOL.md. Parsing throws FormatException on any
// shape the protocol does not allow, so a bad payload never half-builds one.

class User {
  const User({
    required this.id,
    required this.email,
    required this.name,
    required this.hasPassword,
    required this.googleLinked,
  });

  factory User.fromJson(Object? json) => switch (json) {
    {
      'id': String id,
      'email': String email,
      'name': String? name,
      'hasPassword': bool hasPassword,
      'googleLinked': bool googleLinked,
    } =>
      User(
        id: id,
        email: email,
        name: name,
        hasPassword: hasPassword,
        googleLinked: googleLinked,
      ),
    _ => throw const FormatException('Not a User'),
  };

  final String id;
  final String email;
  final String? name;
  final bool hasPassword;
  final bool googleLinked;

  Map<String, Object?> toJson() => {
    'id': id,
    'email': email,
    'name': name,
    'hasPassword': hasPassword,
    'googleLinked': googleLinked,
  };
}

class Device {
  const Device({
    required this.id,
    required this.name,
    required this.platform,
    required this.kind,
    required this.online,
    required this.lastSeenAt,
    required this.current,
    required this.capabilities,
  });

  factory Device.fromJson(Object? json) => switch (json) {
    {
      'id': String id,
      'name': String name,
      'platform': String platform,
      'kind': String kind,
      'online': bool online,
      'lastSeenAt': int lastSeenAt,
      'current': bool current,
      'capabilities': List<Object?> capabilities,
    } =>
      Device(
        id: id,
        name: name,
        platform: platform,
        kind: kind,
        online: online,
        lastSeenAt: DateTime.fromMillisecondsSinceEpoch(lastSeenAt),
        current: current,
        capabilities: capabilities.whereType<String>().toSet(),
      ),
    _ => throw const FormatException('Not a Device'),
  };

  final String id;
  final String name;

  /// windows | macos | linux | android | ios
  final String platform;

  /// desktop | mobile
  final String kind;
  final bool online;
  final DateTime lastSeenAt;

  /// True only for this phone.
  final bool current;

  /// What the device handles; empty while it is offline.
  final Set<String> capabilities;

  bool get isComputer => kind == 'desktop';
}

class DeviceInfo {
  const DeviceInfo({
    required this.name,
    required this.platform,
    required this.kind,
    required this.appVersion,
  });

  final String name;
  final String platform;
  final String kind;
  final String appVersion;

  Map<String, Object?> toJson() => {
    'name': name,
    'platform': platform,
    'kind': kind,
    'appVersion': appVersion,
  };
}

class AuthResponse {
  const AuthResponse({
    required this.token,
    required this.user,
    required this.device,
  });

  factory AuthResponse.fromJson(Object? json) => switch (json) {
    {'token': String token, 'user': Object user, 'device': Object device} =>
      AuthResponse(
        token: token,
        user: User.fromJson(user),
        device: Device.fromJson(device),
      ),
    _ => throw const FormatException('Not an AuthResponse'),
  };

  final String token;
  final User user;
  final Device device;
}
