import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';

const _kBackground = Color(0xFF0F0F1A);
const _kSurfaceDark = Color(0xFF1A1A2E);
const _kSurfaceMid = Color(0xFF16213E);
const _kTextPrimary = Color(0xFFE8E8F0);
const _kTextSecondary = Color(0xFF8A8AA0);
const _kNeonCyan = Color(0xFF00F5FF);
const _kSuccess = Color(0xFF00E676);
const _kAmber = Color(0xFFFFB300);
const _kGlassBorder = Color(0x1AFFFFFF);

/// A scrollable list of decrypted DIDComm messages.
class MessageList extends StatelessWidget {
  final List<DecryptedMsg> messages;
  final ValueChanged<DecryptedMsg>? onMessageTap;

  const MessageList({
    super.key,
    required this.messages,
    this.onMessageTap,
  });

  @override
  Widget build(BuildContext context) {
    if (messages.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              LucideIcons.inbox,
              size: 32,
              color: _kTextSecondary.withValues(alpha: 0.5),
            ),
            const SizedBox(height: 8),
            Text(
              'No messages yet',
              style: GoogleFonts.inter(
                fontSize: 14,
                color: _kTextSecondary,
              ),
            ),
          ],
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(LucideIcons.mail, size: 16, color: _kNeonCyan.withValues(alpha: 0.8)),
            const SizedBox(width: 8),
            Text(
              'MESSAGES',
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: _kTextSecondary,
                letterSpacing: 1.2,
              ),
            ),
            const Spacer(),
            Text(
              '${messages.length}',
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: _kNeonCyan,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        ...messages.map((msg) => _MessageTile(
              message: msg,
              onTap: () => onMessageTap?.call(msg),
            )),
      ],
    );
  }
}

class _MessageTile extends StatelessWidget {
  final DecryptedMsg message;
  final VoidCallback? onTap;

  const _MessageTile({required this.message, this.onTap});

  bool get _isAuthRequest =>
      message.msgType.contains('payment-auth-request');

  IconData get _icon =>
      _isAuthRequest ? LucideIcons.shieldAlert : LucideIcons.mail;

  Color get _accentColor => _isAuthRequest ? _kAmber : _kNeonCyan;

  String get _title {
    if (_isAuthRequest) return 'Payment Request';
    if (message.msgType.contains('list-sync')) return 'List Sync';
    return 'Message';
  }

  String get _subtitle {
    if (message.paymentId != null) {
      return 'Payment: ${message.paymentId!.substring(0, message.paymentId!.length > 12 ? 12 : message.paymentId!.length)}...';
    }
    return message.msgType.split('/').last;
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
          decoration: BoxDecoration(
            color: _kSurfaceDark.withValues(alpha: 0.5),
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: _kGlassBorder),
          ),
          child: Row(
            children: [
              Container(
                width: 36,
                height: 36,
                decoration: BoxDecoration(
                  color: _accentColor.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Icon(_icon, size: 18, color: _accentColor),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      _title,
                      style: GoogleFonts.inter(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: _kTextPrimary,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      _subtitle,
                      style: GoogleFonts.inter(
                        fontSize: 11,
                        color: _kTextSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              Icon(LucideIcons.chevronRight, size: 16, color: _kTextSecondary),
            ],
          ),
        ),
      ),
    );
  }
}
