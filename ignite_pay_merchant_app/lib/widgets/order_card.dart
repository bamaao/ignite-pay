import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';

class OrderCard extends StatelessWidget {
  final PaymentOrder order;
  final VoidCallback? onTap;

  const OrderCard({super.key, required this.order, this.onTap});

  @override
  Widget build(BuildContext context) {
    final statusColor = _statusColor(order.status);
    final statusLabel = _statusLabel(order.status);
    final displayAmount = (order.amount.toDouble() / 1_000_000_000).toStringAsFixed(2);
    final shortId = order.orderId.length > 8
        ? order.orderId.substring(0, 8)
        : order.orderId;
    final time = DateTime.fromMillisecondsSinceEpoch(order.createdAt * 1000);
    final timeStr = '${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}';

    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: glassDecoration(),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Text('$displayAmount USDC', style: monoValue(15)),
                      const Spacer(),
                      _StatusBadge(color: statusColor, label: statusLabel),
                    ],
                  ),
                  const SizedBox(height: 4),
                  Text(order.description.isEmpty ? '无描述' : order.description,
                      style: cardSubtitle()),
                  const SizedBox(height: 2),
                  Text('$timeStr  #$shortId',
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 10,
                        color: kTextTertiary,
                      )),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _StatusBadge extends StatelessWidget {
  final Color color;
  final String label;
  const _StatusBadge({required this.color, required this.label});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: color.withValues(alpha: 0.3)),
      ),
      child: Text(label,
          style: GoogleFonts.inter(
            fontSize: 10,
            fontWeight: FontWeight.w600,
            color: color,
          )),
    );
  }
}

Color _statusColor(String status) {
  switch (status) {
    case 'confirmed':
      return kSuccess;
    case 'pending':
      return kPending;
    case 'failed':
      return kDanger;
    case 'expired':
      return kTextTertiary;
    default:
      return kTextSecondary;
  }
}

String _statusLabel(String status) {
  switch (status) {
    case 'confirmed':
      return '已确认';
    case 'pending':
      return '待确认';
    case 'failed':
      return '失败';
    case 'expired':
      return '已过期';
    default:
      return status;
  }
}
