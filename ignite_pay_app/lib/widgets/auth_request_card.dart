import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';

const _kBackground = Color(0xFF0F0F1A);
const _kSurfaceDark = Color(0xFF1A1A2E);
const _kSurfaceMid = Color(0xFF16213E);
const _kTextPrimary = Color(0xFFE8E8F0);
const _kTextSecondary = Color(0xFF8A8AA0);
const _kSuccess = Color(0xFF00E676);
const _kAmber = Color(0xFFFFB300);
const _kDanger = Color(0xFFFF5252);
const _kGlassBorder = Color(0x1AFFFFFF);

/// Card showing a pending payment authorization request.
class AuthRequestCard extends StatelessWidget {
  final AuthRequest request;
  final VoidCallback? onApprove;
  final VoidCallback? onReject;

  const AuthRequestCard({
    super.key,
    required this.request,
    this.onApprove,
    this.onReject,
  });

  String get _solAmount {
    final sol = request.amount / 1000000000.0;
    return '${sol.toStringAsFixed(2)} SOL';
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(18),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.7),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: _kAmber.withValues(alpha: 0.3)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 32,
                height: 32,
                decoration: BoxDecoration(
                  color: _kAmber.withValues(alpha: 0.15),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: const Icon(LucideIcons.shieldAlert, size: 18, color: _kAmber),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  'AUTHORIZATION REQUIRED',
                  style: GoogleFonts.inter(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: _kAmber,
                    letterSpacing: 1.0,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          _InfoRow(
            label: 'Amount',
            value: _solAmount,
            valueColor: _kTextPrimary,
          ),
          const SizedBox(height: 8),
          _InfoRow(
            label: 'Merchant',
            value: request.merchantDid.length > 30
                ? '${request.merchantDid.substring(0, 24)}...'
                : request.merchantDid,
            valueColor: _kTextSecondary,
          ),
          if (request.description.isNotEmpty) ...[
            const SizedBox(height: 8),
            _InfoRow(
              label: 'Description',
              value: request.description,
              valueColor: _kTextSecondary,
            ),
          ],
          const SizedBox(height: 16),
          Row(
            children: [
              Expanded(
                child: GestureDetector(
                  onTap: onReject,
                  child: Container(
                    padding: const EdgeInsets.symmetric(vertical: 10),
                    decoration: BoxDecoration(
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: _kDanger.withValues(alpha: 0.3)),
                      color: _kDanger.withValues(alpha: 0.08),
                    ),
                    child: Center(
                      child: Text(
                        'Decline',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: _kDanger,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: GestureDetector(
                  onTap: onApprove,
                  child: Container(
                    padding: const EdgeInsets.symmetric(vertical: 10),
                    decoration: BoxDecoration(
                      borderRadius: BorderRadius.circular(10),
                      color: _kSuccess.withValues(alpha: 0.15),
                      border: Border.all(color: _kSuccess.withValues(alpha: 0.3)),
                    ),
                    child: Center(
                      child: Text(
                        'Approve',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: _kSuccess,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  final Color valueColor;

  const _InfoRow({
    required this.label,
    required this.value,
    required this.valueColor,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 90,
          child: Text(
            label,
            style: GoogleFonts.inter(
              fontSize: 11,
              color: _kTextSecondary,
              fontWeight: FontWeight.w500,
            ),
          ),
        ),
        Expanded(
          child: Text(
            value,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 12,
              color: valueColor,
            ),
          ),
        ),
      ],
    );
  }
}
