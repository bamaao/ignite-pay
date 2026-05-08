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
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/challenge_screen.dart';
import 'package:provider/provider.dart';

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openMessages(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const MessagesScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Messages Screen
// ---------------------------------------------------------------------------
class MessagesScreen extends StatefulWidget {
  const MessagesScreen({super.key});

  @override
  State<MessagesScreen> createState() => _MessagesScreenState();
}

class _MessagesScreenState extends State<MessagesScreen> {
  _MsgFilter _filter = _MsgFilter.all;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
              child: const PageHeader(
                title: 'Messages',
                subtitle: 'DIDComm encrypted messages',
              ),
            ),
            const SizedBox(height: 16),
            // Filter chips
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20),
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: _MsgFilter.values.map((f) {
                    final selected = f == _filter;
                    return Padding(
                      padding: const EdgeInsets.only(right: 8),
                      child: GestureDetector(
                        onTap: () => setState(() => _filter = f),
                        child: Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 14, vertical: 7),
                          decoration: BoxDecoration(
                            color: selected
                                ? kNeonCyan.withValues(alpha: 0.12)
                                : kSurfaceDark,
                            borderRadius: BorderRadius.circular(20),
                            border: Border.all(
                              color: selected
                                  ? kNeonCyan.withValues(alpha: 0.3)
                                  : kBorder,
                            ),
                          ),
                          child: Text(
                            f.label,
                            style: GoogleFonts.inter(
                              fontSize: 12,
                              fontWeight: FontWeight.w600,
                              color: selected ? kNeonCyan : kTextSecondary,
                            ),
                          ),
                        ),
                      ),
                    );
                  }).toList(),
                ),
              ),
            ),
            const SizedBox(height: 16),
            // Message list
            Expanded(
              child: Consumer<DidcommService>(
                builder: (context, svc, _) {
                  final all = svc.messages;
                  final filtered = _filter == _MsgFilter.all
                      ? all
                      : all.where((m) {
                          switch (_filter) {
                            case _MsgFilter.payment:
                              return m.msgType
                                  .contains('payment-auth-request');
                            case _MsgFilter.listSync:
                              return m.msgType.contains('list-sync');
                            case _MsgFilter.connection:
                              return m.msgType.contains('connection');
                            case _MsgFilter.all:
                              return true;
                          }
                        }).toList();

                  if (filtered.isEmpty) {
                    return _EmptyState(onRefresh: () => _refresh(svc));
                  }

                  return RefreshIndicator(
                    color: kNeonCyan,
                    backgroundColor: kSurfaceDark,
                    onRefresh: () => _refresh(svc),
                    child: ListView.separated(
                      padding: const EdgeInsets.symmetric(horizontal: 20),
                      itemCount: filtered.length,
                      separatorBuilder: (_, a) => const SizedBox(height: 6),
                      itemBuilder: (context, index) {
                        final msg = filtered[filtered.length - 1 - index];
                        return _MessageTile(msg: msg);
                      },
                    ),
                  );
                },
              ),
            ),
            const SizedBox(height: 20),
          ],
        ),
      ),
    );
  }

  Future<void> _refresh(DidcommService svc) async {
    try {
      await svc.connectToMediator(svc.mediatorWsUrl);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Messages refreshed',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 2),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kDanger,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Failed to refresh: $e',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 3),
          ),
        );
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Filter Enum
// ---------------------------------------------------------------------------
enum _MsgFilter {
  all('All'),
  payment('Payment'),
  listSync('List Sync'),
  connection('Connection');

  final String label;
  const _MsgFilter(this.label);
}

// ---------------------------------------------------------------------------
// Empty State
// ---------------------------------------------------------------------------
class _EmptyState extends StatelessWidget {
  final VoidCallback onRefresh;
  const _EmptyState({required this.onRefresh});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(LucideIcons.inbox, size: 40, color: kTextTertiary),
          const SizedBox(height: 14),
          Text(
            'No messages yet',
            style: GoogleFonts.inter(
              fontSize: 15,
              fontWeight: FontWeight.w600,
              color: kTextSecondary,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            'Messages will appear here when your\nMCP agent sends payment requests',
            textAlign: TextAlign.center,
            style: GoogleFonts.inter(
              fontSize: 12,
              color: kTextTertiary,
              height: 1.5,
            ),
          ),
          const SizedBox(height: 20),
          GestureDetector(
            onTap: onRefresh,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 10),
              decoration: BoxDecoration(
                color: kNeonCyan.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(20),
                border: Border.all(color: kNeonCyan.withValues(alpha: 0.25)),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(LucideIcons.refreshCw, size: 14, color: kNeonCyan),
                  const SizedBox(width: 8),
                  Text(
                    'Check for messages',
                    style: GoogleFonts.inter(
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      color: kNeonCyan,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Message Tile
// ---------------------------------------------------------------------------
class _MessageTile extends StatelessWidget {
  final DecryptedMsg msg;
  const _MessageTile({required this.msg});

  IconData get _icon => switch (_msgType) {
        _MsgType.payment => LucideIcons.creditCard,
        _MsgType.listSync => LucideIcons.listChecks,
        _MsgType.connection => LucideIcons.link,
        _MsgType.other => LucideIcons.mail,
      };

  Color get _iconColor => switch (_msgType) {
        _MsgType.payment => kAmber,
        _MsgType.listSync => kPurple,
        _MsgType.connection => kCyan,
        _MsgType.other => kTextSecondary,
      };

  String get _typeLabel => switch (_msgType) {
        _MsgType.payment => 'Payment Request',
        _MsgType.listSync => 'List Sync',
        _MsgType.connection => 'Connection',
        _MsgType.other => 'Message',
      };

  _MsgType get _msgType {
    if (msg.msgType.contains('payment-auth-request')) return _MsgType.payment;
    if (msg.msgType.contains('list-sync')) return _MsgType.listSync;
    if (msg.msgType.contains('connection')) return _MsgType.connection;
    return _MsgType.other;
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: () => _handleTap(context),
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: glassDecoration(),
        child: Row(
          children: [
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: _iconColor.withValues(alpha: 0.08),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(_icon, size: 18, color: _iconColor),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    _typeLabel,
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                      color: kTextPrimary,
                    ),
                  ),
                  const SizedBox(height: 3),
                  if (msg.merchantDid != null)
                    Text(
                      _shortenDid(msg.merchantDid!),
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 11,
                        color: kTextSecondary,
                      ),
                    ),
                  if (msg.description != null && msg.description!.isNotEmpty)
                    Text(
                      msg.description!,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: GoogleFonts.inter(
                        fontSize: 11,
                        color: kTextTertiary,
                      ),
                    ),
                ],
              ),
            ),
            if (_msgType == _MsgType.payment && msg.amount != null) ...[
              const SizedBox(width: 8),
              Text(
                '${(msg.amount! / 1e9).toStringAsFixed(4)} SOL',
                style: GoogleFonts.jetBrainsMono(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: kAmber,
                ),
              ),
            ],
            const SizedBox(width: 6),
            Icon(LucideIcons.chevronRight,
                size: 16, color: kTextTertiary),
          ],
        ),
      ),
    );
  }

  String _shortenDid(String did) {
    if (did.length > 30) return '${did.substring(0, 20)}...${did.substring(did.length - 6)}';
    return did;
  }

  void _handleTap(BuildContext context) {
    if (_msgType == _MsgType.payment) {
      final request = AuthRequest(
        paymentId: msg.paymentId ?? '',
        merchantDid: msg.merchantDid ?? '',
        amount: msg.amount ?? 0,
        description: msg.description ?? '',
      );
      showX402Challenge(context, request: request);
    } else {
      // Show raw body in a dialog
      showDialog(
        context: context,
        builder: (ctx) => _MessageDetailDialog(msg: msg),
      );
    }
  }
}

// ---------------------------------------------------------------------------
// Message Detail Dialog
// ---------------------------------------------------------------------------
class _MessageDetailDialog extends StatelessWidget {
  final DecryptedMsg msg;
  const _MessageDetailDialog({required this.msg});

  @override
  Widget build(BuildContext context) {
    return Dialog(
      backgroundColor: kSurfaceDark,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text(
                  'Message Detail',
                  style: GoogleFonts.inter(
                    fontSize: 16,
                    fontWeight: FontWeight.w700,
                    color: kTextPrimary,
                  ),
                ),
                const Spacer(),
                GestureDetector(
                  onTap: () => Navigator.of(context).pop(),
                  child: Icon(LucideIcons.x, size: 20, color: kTextSecondary),
                ),
              ],
            ),
            const SizedBox(height: 16),
            _field('Type', msg.msgType),
            if (msg.paymentId != null) _field('Payment ID', msg.paymentId!),
            if (msg.merchantDid != null) _field('Merchant', msg.merchantDid!),
            if (msg.amount != null)
              _field('Amount', '${(msg.amount! / 1e9).toStringAsFixed(6)} SOL'),
            if (msg.description != null) _field('Description', msg.description!),
            if (msg.listCid != null) _field('List CID', msg.listCid!),
            if (msg.listType != null) _field('List Type', msg.listType!),
            if (msg.label != null) _field('Label', msg.label!),
            const SizedBox(height: 12),
            Text('RAW BODY', style: sectionLabel()),
            const SizedBox(height: 6),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: kSurfaceMid,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: kBorder),
              ),
              child: SelectableText(
                msg.rawBody,
                style: GoogleFonts.jetBrainsMono(
                  fontSize: 10,
                  color: kTextSecondary,
                  height: 1.4,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _field(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label.toUpperCase(), style: sectionLabel()),
          const SizedBox(height: 2),
          Text(value, style: monoValue(12)),
        ],
      ),
    );
  }
}

enum _MsgType { payment, listSync, connection, other }
