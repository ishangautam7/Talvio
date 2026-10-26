import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import '../../core/session_store.dart';

class RecentCommand {
  const RecentCommand({
    required this.said,
    required this.feedback,
    required this.status,
    required this.at,
    this.id,
  });

  /// The command as typed, or as the computer transcribed it.
  final String said;
  final String feedback;
  final String status;
  final DateTime at;

  /// While it waits for a confirmation or an answer: what the computer
  /// called it, so the follow-up's result can update this entry.
  final int? id;

  Map<String, Object?> toJson() => {
    'said': said,
    'feedback': feedback,
    'status': status,
    'at': at.millisecondsSinceEpoch,
    'id': ?id,
  };
}

/// The last few commands, on this phone only (removed on sign-out).
class RecentCommands {
  RecentCommands(this._prefs) : _items = _load(_prefs);

  static const limit = 10;
  static const _key = '${accountPrefsPrefix}command.recent';

  final SharedPreferences _prefs;
  List<RecentCommand> _items;

  /// Newest first.
  List<RecentCommand> get items => _items;

  Future<void> add(RecentCommand item) =>
      _save([item, ..._items].take(limit).toList());

  /// The follow-up to the command waiting under [waitingId] ended as
  /// [status]; [id] is set when it waits again.
  Future<void> update(
    int waitingId, {
    required String feedback,
    required String status,
    int? id,
  }) {
    final index = _items.indexWhere((item) => item.id == waitingId);
    if (index < 0) return Future.value();
    final old = _items[index];
    return _save([
      ..._items.take(index),
      RecentCommand(
        said: old.said,
        feedback: feedback.isEmpty ? old.feedback : feedback,
        status: status,
        at: old.at,
        id: id,
      ),
      ..._items.skip(index + 1),
    ]);
  }

  Future<void> _save(List<RecentCommand> items) async {
    _items = items;
    await _prefs.setString(
      _key,
      jsonEncode([for (final item in items) item.toJson()]),
    );
  }

  static List<RecentCommand> _load(SharedPreferences prefs) {
    final raw = prefs.getString(_key);
    if (raw == null) return const [];
    try {
      final list = jsonDecode(raw);
      if (list is! List<Object?>) return const [];
      return [
        for (final item in list)
          if (item case {
            'said': String said,
            'feedback': String feedback,
            'status': String status,
            'at': int at,
          })
            RecentCommand(
              said: said,
              feedback: feedback,
              status: status,
              at: DateTime.fromMillisecondsSinceEpoch(at),
              id: item['id'] is int ? item['id']! as int : null,
            ),
      ];
    } on FormatException {
      return const [];
    }
  }
}
