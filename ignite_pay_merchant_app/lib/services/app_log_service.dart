// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

import 'dart:collection';
import 'package:flutter/foundation.dart';

/// Severity level for log entries.
enum LogLevel { info, warn, error }

/// A single log entry.
class LogEntry {
  final DateTime timestamp;
  final LogLevel level;
  final String tag;
  final String message;

  LogEntry({
    required this.timestamp,
    required this.level,
    required this.tag,
    required this.message,
  });

  String get levelStr {
    switch (level) {
      case LogLevel.info:
        return 'INFO';
      case LogLevel.warn:
        return 'WARN';
      case LogLevel.error:
        return 'ERROR';
    }
  }

  @override
  String toString() =>
      '${timestamp.toIso8601String()} [$levelStr] [$tag] $message';
}

/// In-memory ring-buffer log service.
/// Captures app events for display in the Log Viewer screen.
class AppLogService extends ChangeNotifier {
  static final AppLogService _instance = AppLogService._internal();
  factory AppLogService() => _instance;
  AppLogService._internal();

  static const int _maxEntries = 500;
  final Queue<LogEntry> _entries = Queue<LogEntry>();

  /// Unmodifiable list of all log entries (newest last).
  List<LogEntry> get entries => List.unmodifiable(_entries);

  int get count => _entries.length;

  void info(String tag, String message) => _add(LogLevel.info, tag, message);
  void warn(String tag, String message) => _add(LogLevel.warn, tag, message);
  void error(String tag, String message) => _add(LogLevel.error, tag, message);

  void _add(LogLevel level, String tag, String message) {
    final entry = LogEntry(
      timestamp: DateTime.now(),
      level: level,
      tag: tag,
      message: message,
    );
    _entries.addLast(entry);
    while (_entries.length > _maxEntries) {
      _entries.removeFirst();
    }
    // Also print to debug console
    debugPrint('[${entry.levelStr}] [$tag] $message');
    notifyListeners();
  }

  /// Clear all log entries.
  void clear() {
    _entries.clear();
    notifyListeners();
  }

  /// Export all entries as a single string.
  String exportText() {
    return _entries.map((e) => e.toString()).join('\n');
  }
}
