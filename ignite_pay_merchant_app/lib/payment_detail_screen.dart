import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';

void openPaymentDetail(BuildContext context, PaymentOrder order) {
  Navigator.of(context).push(PageRouteBuilder(
    pageBuilder: (_, __, ___) => PaymentDetailScreen(order: order),
    transitionsBuilder: (_, anim, __, child) =>
        SlideTransition(position: Tween(begin: const Offset(1, 0), end: Offset.zero).animate(anim), child: child),
  ));
}

class PaymentDetailScreen extends StatelessWidget {
  final PaymentOrder order;
  const PaymentDetailScreen({super.key, required this.order});

  @override
  Widget build(BuildContext context) {
    final displayAmount = (order.amount.toDouble() / 1_000_000_000).toStringAsFixed(2);
    final statusColor = _statusColor(order.status);
    final statusLabel = _statusLabel(order.status);
    final created = DateTime.fromMillisecondsSinceEpoch(order.createdAt * 1000);
    final confirmed = order.confirmedAt != null
        ? DateTime.fromMillisecondsSinceEpoch(order.confirmedAt! * 1000)
        : null;

    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const PageHeader(title: '收款详情'),
                const SizedBox(height: 24),
                // Amount + status
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(24),
                  decoration: glassCardDecoration(),
                  child: Column(
                    children: [
                      Text('$displayAmount USDC',
                          style: GoogleFonts.jetBrainsMono(
                            fontSize: 28, fontWeight: FontWeight.w700, color: kTextPrimary,
                          )),
                      const SizedBox(height: 8),
                      Container(
                        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
                        decoration: BoxDecoration(
                          color: statusColor.withValues(alpha: 0.12),
                          borderRadius: BorderRadius.circular(10),
                          border: Border.all(color: statusColor.withValues(alpha: 0.3)),
                        ),
                        child: Text(statusLabel,
                            style: GoogleFonts.inter(
                              fontSize: 12, fontWeight: FontWeight.w600, color: statusColor,
                            )),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 16),
                // Order info
                const SectionLabel(text: 'ORDER INFO'),
                const SizedBox(height: 8),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(14),
                  decoration: glassDecoration(),
                  child: Column(
                    children: [
                      _InfoRow(label: '订单号', value: order.orderId, mono: true, copyable: true),
                      const SizedBox(height: 10),
                      _InfoRow(label: '描述', value: order.description.isEmpty ? '无' : order.description),
                      const SizedBox(height: 10),
                      _InfoRow(label: 'Hub', value: order.hubEndpoint, mono: true),
                      const SizedBox(height: 10),
                      _InfoRow(label: '创建时间', value: _formatTime(created)),
                      if (confirmed != null) ...[
                        const SizedBox(height: 10),
                        _InfoRow(label: '确认时间', value: _formatTime(confirmed)),
                      ],
                    ],
                  ),
                ),
                // Channel info (only if confirmed)
                if (order.channelId != null) ...[
                  const SizedBox(height: 16),
                  const SectionLabel(text: 'CHANNEL INFO'),
                  const SizedBox(height: 8),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(14),
                    decoration: glassDecoration(),
                    child: Column(
                      children: [
                        _InfoRow(label: '通道 ID', value: order.channelId!, mono: true, copyable: true),
                        if (order.leafIndex != null) ...[
                          const SizedBox(height: 10),
                          _InfoRow(label: '叶子索引', value: '${order.leafIndex}'),
                        ],
                        if (order.sequence != null) ...[
                          const SizedBox(height: 10),
                          _InfoRow(label: '序列号', value: '${order.sequence}'),
                        ],
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  final bool mono;
  final bool copyable;

  const _InfoRow({
    required this.label,
    required this.value,
    this.mono = false,
    this.copyable = false,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 72,
          child: Text(label, style: cardSubtitle()),
        ),
        Expanded(
          child: Row(
            children: [
              Expanded(
                child: Text(
                  value,
                  style: mono ? monoValue(12) : GoogleFonts.inter(fontSize: 13, color: kTextPrimary),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (copyable)
                GestureDetector(
                  onTap: () => Clipboard.setData(ClipboardData(text: value)),
                  child: const Padding(
                    padding: EdgeInsets.only(left: 4),
                    child: Icon(LucideIcons.copy, size: 14, color: kTextSecondary),
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }
}

String _formatTime(DateTime t) {
  return '${t.year}-${t.month.toString().padLeft(2, '0')}-${t.day.toString().padLeft(2, '0')} '
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}:${t.second.toString().padLeft(2, '0')}';
}

Color _statusColor(String status) {
  switch (status) {
    case 'confirmed': return kSuccess;
    case 'pending': return kPending;
    case 'failed': return kDanger;
    default: return kTextSecondary;
  }
}

String _statusLabel(String status) {
  switch (status) {
    case 'confirmed': return '已确认';
    case 'pending': return '待确认';
    case 'failed': return '失败';
    case 'expired': return '已过期';
    default: return status;
  }
}
