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

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/app_log_service.dart';

void openLogViewer(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const LogViewerScreen(),
      ),
    ),
  );
}

class LogViewerScreen extends StatefulWidget {
  const LogViewerScreen({super.key});

  @override
  State<LogViewerScreen> createState() => _LogViewerScreenState();
}

class _LogViewerScreenState extends State<LogViewerScreen> {
  LogLevel? _filter;
  final _scrollController = ScrollController();
  bool _autoScroll = true;

  @override
  void dispose() {
    _scrollController.dispose();
    super.dispose();
  }

  void _scrollToBottom() {
    if (_autoScroll && _scrollController.hasClients) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
      });
    }
  }

  Color _levelColor(LogLevel level) {
    switch (level) {
      case LogLevel.info:
        return kNeonCyan;
      case LogLevel.warn:
        return kAmber;
      case LogLevel.error:
        return kDanger;
    }
  }

  IconData _levelIcon(LogLevel level) {
    switch (level) {
      case LogLevel.info:
        return LucideIcons.info;
      case LogLevel.warn:
        return LucideIcons.alertTriangle;
      case LogLevel.error:
        return LucideIcons.alertCircle;
    }
  }

  @override
  Widget build(BuildContext context) {
    final log = AppLogService();
    return AnimatedBuilder(
      animation: log,
      builder: (context, _) {
        final filtered = _filter == null
            ? log.entries
            : log.entries.where((e) => e.level == _filter).toList();

        _scrollToBottom();

        return Scaffold(
          backgroundColor: kBackground,
          body: SafeArea(
            child: Column(
              children: [
                // Header
                Padding(
                  padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
                  child: Row(
                    children: [
                      const BackButtonGlass(),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text('Logs', style: GoogleFonts.inter(
                              fontSize: 18, fontWeight: FontWeight.w700, color: kTextPrimary,
                            )),
                            Text('${filtered.length} entries', style: cardSubtitle()),
                          ],
                        ),
                      ),
                      // Filter chips
                      Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          _filterChip(null, 'All', kTextPrimary),
                          const SizedBox(width: 4),
                          _filterChip(LogLevel.info, 'I', kNeonCyan),
                          const SizedBox(width: 4),
                          _filterChip(LogLevel.warn, 'W', kAmber),
                          const SizedBox(width: 4),
                          _filterChip(LogLevel.error, 'E', kDanger),
                        ],
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 12),

                // Action bar
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  child: Row(
                    children: [
                      _actionButton(
                        icon: LucideIcons.copy,
                        label: 'Copy All',
                        onTap: () {
                          Clipboard.setData(ClipboardData(text: log.exportText()));
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                              backgroundColor: kSuccess,
                              behavior: SnackBarBehavior.floating,
                              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                              margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                              content: Text('Logs copied to clipboard',
                                  style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
                            ),
                          );
                        },
                      ),
                      const SizedBox(width: 8),
                      _actionButton(
                        icon: LucideIcons.trash,
                        label: 'Clear',
                        onTap: () => log.clear(),
                      ),
                      const Spacer(),
                      GestureDetector(
                        onTap: () => setState(() => _autoScroll = !_autoScroll),
                        child: Row(
                          children: [
                            Icon(
                              LucideIcons.arrowDown,
                              size: 14,
                              color: _autoScroll ? kNeonCyan : kTextTertiary,
                            ),
                            const SizedBox(width: 4),
                            Text('Auto', style: GoogleFonts.inter(
                              fontSize: 11,
                              color: _autoScroll ? kNeonCyan : kTextTertiary,
                            )),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 8),

                // Log list
                Expanded(
                  child: filtered.isEmpty
                      ? Center(
                          child: Text('No log entries', style: GoogleFonts.inter(
                            fontSize: 14, color: kTextTertiary,
                          )),
                        )
                      : ListView.builder(
                          controller: _scrollController,
                          padding: const EdgeInsets.symmetric(horizontal: 16),
                          itemCount: filtered.length,
                          itemExtent: null,
                          itemBuilder: (context, index) {
                            final entry = filtered[index];
                            final color = _levelColor(entry.level);
                            return Padding(
                              padding: const EdgeInsets.symmetric(vertical: 2),
                              child: Row(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  // Timestamp
                                  SizedBox(
                                    width: 52,
                                    child: Text(
                                      '${entry.timestamp.hour.toString().padLeft(2, '0')}:'
                                      '${entry.timestamp.minute.toString().padLeft(2, '0')}:'
                                      '${entry.timestamp.second.toString().padLeft(2, '0')}',
                                      style: GoogleFonts.jetBrainsMono(
                                        fontSize: 9, color: kTextTertiary,
                                      ),
                                    ),
                                  ),
                                  // Level badge
                                  Container(
                                    width: 14,
                                    height: 14,
                                    margin: const EdgeInsets.only(top: 1),
                                    decoration: BoxDecoration(
                                      color: color.withValues(alpha: 0.2),
                                      borderRadius: BorderRadius.circular(3),
                                    ),
                                    child: Icon(
                                      _levelIcon(entry.level),
                                      size: 10,
                                      color: color,
                                    ),
                                  ),
                                  const SizedBox(width: 6),
                                  // Tag
                                  SizedBox(
                                    width: 56,
                                    child: Text(
                                      entry.tag,
                                      style: GoogleFonts.jetBrainsMono(
                                        fontSize: 9,
                                        color: kTextSecondary,
                                        fontWeight: FontWeight.w600,
                                      ),
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                  const SizedBox(width: 4),
                                  // Message
                                  Expanded(
                                    child: Text(
                                      entry.message,
                                      style: GoogleFonts.jetBrainsMono(
                                        fontSize: 10,
                                        color: kTextPrimary,
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                            );
                          },
                        ),
                ),
                const SizedBox(height: 16),
              ],
            ),
          ),
        );
      },
    );
  }

  Widget _filterChip(LogLevel? level, String label, Color color) {
    final active = _filter == level;
    return GestureDetector(
      onTap: () => setState(() => _filter = active ? null : level),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: active ? color.withValues(alpha: 0.15) : Colors.transparent,
          borderRadius: BorderRadius.circular(6),
          border: Border.all(
            color: active ? color.withValues(alpha: 0.4) : kBorder,
          ),
        ),
        child: Text(
          label,
          style: GoogleFonts.inter(
            fontSize: 10,
            fontWeight: FontWeight.w600,
            color: active ? color : kTextTertiary,
          ),
        ),
      ),
    );
  }

  Widget _actionButton({
    required IconData icon,
    required String label,
    required VoidCallback onTap,
  }) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: kSurfaceDark,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: kBorder),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 13, color: kTextSecondary),
            const SizedBox(width: 6),
            Text(label, style: GoogleFonts.inter(
              fontSize: 11, fontWeight: FontWeight.w600, color: kTextSecondary,
            )),
          ],
        ),
      ),
    );
  }
}
