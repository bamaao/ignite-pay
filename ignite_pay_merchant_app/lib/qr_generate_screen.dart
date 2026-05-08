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

import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/widgets/amount_input.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';
import 'package:provider/provider.dart';

void openQrGenerate(BuildContext context) {
  Navigator.of(context).push(PageRouteBuilder(
    pageBuilder: (_, __, ___) => const QrGenerateScreen(),
    transitionsBuilder: (_, anim, __, child) =>
        SlideTransition(position: Tween(begin: const Offset(1, 0), end: Offset.zero).animate(anim), child: child),
  ));
}

class QrGenerateScreen extends StatefulWidget {
  const QrGenerateScreen({super.key});

  @override
  State<QrGenerateScreen> createState() => _QrGenerateScreenState();
}

class _QrGenerateScreenState extends State<QrGenerateScreen> {
  String _amountText = '';
  final _descController = TextEditingController();
  String? _qrText;
  String? _orderId;
  bool _generating = false;
  String _status = 'idle'; // idle, waiting, confirmed
  Timer? _fallbackPollTimer;
  StreamSubscription<PaymentConfirmation>? _confirmationSub;

  @override
  void dispose() {
    _descController.dispose();
    _fallbackPollTimer?.cancel();
    _confirmationSub?.cancel();
    super.dispose();
  }

  BigInt get _amountLamports {
    final parsed = double.tryParse(_amountText);
    if (parsed == null || parsed <= 0) return BigInt.zero;
    return BigInt.from((parsed * 1_000_000_000).round());
  }

  Future<void> _generate() async {
    if (_amountLamports == BigInt.zero) return;

    setState(() {
      _generating = true;
      _status = 'idle';
    });

    try {
      final svc = context.read<MerchantService>();
      _qrText = await svc.generatePaymentQr(_amountLamports, _descController.text);
      // Extract order ID from the last created order
      final latestOrder = svc.orders.firstOrNull;
      _orderId = latestOrder?.orderId;

      setState(() {
        _generating = false;
        _status = 'waiting';
      });

      _startWaitingForConfirmation();
    } catch (e) {
      setState(() => _generating = false);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          backgroundColor: kDanger,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('生成失败: $e', style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        ));
      }
    }
  }

  void _startWaitingForConfirmation() {
    // Listen for push-based confirmation
    final pushSvc = context.read<MerchantPushService>();
    _confirmationSub?.cancel();
    _confirmationSub = pushSvc.confirmations.listen((confirmation) {
      if (!mounted || _orderId == null) return;
      if (confirmation.orderId == _orderId) {
        _fallbackPollTimer?.cancel();
        HapticFeedback.mediumImpact();
        setState(() => _status = 'confirmed');
      }
    });

    // Lightweight fallback polling (push may be delayed)
    _fallbackPollTimer?.cancel();
    _fallbackPollTimer = Timer.periodic(const Duration(seconds: 5), (_) async {
      if (!mounted || _orderId == null) return;
      final svc = context.read<MerchantService>();
      await svc.refreshOrders();
      final order = svc.orders.where((o) => o.orderId == _orderId).firstOrNull;
      if (order != null && order.status == 'confirmed') {
        _fallbackPollTimer?.cancel();
        HapticFeedback.mediumImpact();
        setState(() => _status = 'confirmed');
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const PageHeader(title: '生成收款码'),
              const SizedBox(height: 24),
              // Amount input
              const SectionLabel(text: '金额 (USDC)'),
              const SizedBox(height: 8),
              AmountInput(onChanged: (v) => setState(() => _amountText = v)),
              const SizedBox(height: 16),
              // Description
              const SectionLabel(text: '描述 (可选)'),
              const SizedBox(height: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 14),
                decoration: BoxDecoration(
                  color: kSurfaceDark,
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(color: kBorder),
                ),
                child: TextField(
                  controller: _descController,
                  style: GoogleFonts.inter(fontSize: 14, color: kTextPrimary),
                  decoration: InputDecoration(
                    hintText: '例如: 咖啡',
                    hintStyle: GoogleFonts.inter(fontSize: 14, color: kTextTertiary),
                    border: InputBorder.none,
                    contentPadding: const EdgeInsets.symmetric(vertical: 12),
                  ),
                ),
              ),
              const SizedBox(height: 20),
              // Generate button
              GestureDetector(
                onTap: _amountLamports != BigInt.zero && !_generating ? _generate : null,
                child: Container(
                  width: double.infinity,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                  decoration: BoxDecoration(
                    gradient: _amountLamports != BigInt.zero && !_generating
                        ? const LinearGradient(colors: [kNeonCyan, kNeonCyanDim])
                        : null,
                    color: _amountLamports != BigInt.zero && !_generating ? null : kSurfaceElevated,
                    borderRadius: BorderRadius.circular(12),
                    boxShadow: _amountLamports != BigInt.zero
                        ? [BoxShadow(color: kNeonCyan.withValues(alpha: 0.25), blurRadius: 16, spreadRadius: 2)]
                        : null,
                  ),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      if (_generating)
                        const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2, color: kBackground))
                      else
                        const Icon(LucideIcons.qrCode, size: 18, color: kBackground),
                      const SizedBox(width: 8),
                      Text(
                        _generating ? '生成中...' : '生成收款码',
                        style: GoogleFonts.inter(
                          fontSize: 14, fontWeight: FontWeight.w700,
                          color: _amountLamports != BigInt.zero ? kBackground : kTextTertiary,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 20),
              // QR display
              if (_qrText != null)
                Expanded(
                  child: Center(
                    child: Container(
                      padding: const EdgeInsets.all(24),
                      decoration: glassCardDecoration(),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (_status == 'confirmed')
                            Container(
                              width: 200, height: 200,
                              decoration: BoxDecoration(
                                color: kSuccess.withValues(alpha: 0.1),
                                borderRadius: BorderRadius.circular(16),
                                border: Border.all(color: kSuccess.withValues(alpha: 0.3)),
                              ),
                              child: const Icon(LucideIcons.checkCircle, size: 64, color: kSuccess),
                            )
                          else
                            QrImageView(
                              data: _qrText!,
                              version: QrVersions.auto,
                              size: 200,
                              backgroundColor: Colors.white,
                              eyeStyle: const QrEyeStyle(eyeShape: QrEyeShape.square),
                            ),
                          const SizedBox(height: 16),
                          Text(
                            _status == 'confirmed' ? '已收款' : '等待收款中...',
                            style: GoogleFonts.inter(
                              fontSize: 15,
                              fontWeight: FontWeight.w600,
                              color: _status == 'confirmed' ? kSuccess : kPending,
                            ),
                          ),
                          if (_amountLamports != BigInt.zero) ...[
                            const SizedBox(height: 4),
                            Text(
                              '${(_amountLamports.toDouble() / 1_000_000_000).toStringAsFixed(2)} USDC',
                              style: monoValue(16),
                            ),
                          ],
                          if (_orderId != null) ...[
                            const SizedBox(height: 8),
                            Text('#${_orderId!.substring(0, 8)}',
                                style: GoogleFonts.jetBrainsMono(fontSize: 11, color: kTextTertiary)),
                          ],
                        ],
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
