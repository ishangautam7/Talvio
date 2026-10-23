import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

import '../../core/session_store.dart';

class RecentDictation {
  const RecentDictation(this.text, this.at);

  final String text;
  final DateTime at;
}

/// The last few dictations, on this phone only (removed on sign-out).
class RecentDictations {
  RecentDictations(this._prefs) : _items = _load(_prefs);

  static const limit = 20;
  static const _key = '${accountPrefsPrefix}dictate.recent';

  final SharedPreferences _prefs;
  List<RecentDictation> _items;

  /// Newest first.
  List<RecentDictation> get items => _items;

  Future<void> add(String text) async {
    _items = [
      RecentDictation(text, DateTime.now()),
      ..._items,
    ].take(limit).toList();
    await _prefs.setString(
      _key,
      jsonEncode([
        for (final item in _items)
          {'text': item.text, 'at': item.at.millisecondsSinceEpoch},
      ]),
    );
  }

  static List<RecentDictation> _load(SharedPreferences prefs) {
    final raw = prefs.getString(_key);
    if (raw == null) return const [];
    try {
      final list = jsonDecode(raw);
      if (list is! List<Object?>) return const [];
      return [
        for (final item in list)
          if (item case {'text': String text, 'at': int at})
            RecentDictation(text, DateTime.fromMillisecondsSinceEpoch(at)),
      ];
    } on FormatException {
      return const [];
    }
  }
}
