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
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/notification_center_screen.dart';
import 'package:ignite_pay_merchant/widgets/order_card.dart';
import 'package:ignite_pay_merchant/qr_generate_screen.dart';
import 'package:ignite_pay_merchant/hub_selection_screen.dart';
import 'package:ignite_pay_merchant/channel_screen.dart';
import 'package:ignite_pay_merchant/payment_detail_screen.dart';
import 'package:provider/provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

class DashboardScreen extends StatelessWidget {
  const DashboardScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: Column(
            children: [
              const _DashboardHeader(),
              const SizedBox(height: 20),
              Expanded(
                child: SingleChildScrollView(
                  child: Column(
                    children: [
                      const _TodaySummary(),
                      const SizedBox(height: 16),
                      _QuickActions(onGenerateQr: () => openQrGenerate(context)),
                      const SizedBox(height: 20),
                      const _RecentOrders(),
                    ],
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

class _DashboardHeader extends StatelessWidget {
  const _DashboardHeader();

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();
    final hasOrders = svc.orders.isNotEmpty;

    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(10),
                gradient: const LinearGradient(
                  colors: [kNeonCyan, kNeonCyanDim],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
              ),
              child: const Icon(LucideIcons.store, size: 20, color: kBackground),
            ),
            const SizedBox(width: 12),
            Text('Ignite Merchant',
                style: GoogleFonts.inter(
                  fontSize: 20,
                  fontWeight: FontWeight.w700,
                  color: kTextPrimary,
                  letterSpacing: -0.5,
                )),
          ],
        ),
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            GestureDetector(
              onTap: () => openNotificationCenter(context),
              child: Container(
                width: 36,
                height: 36,
                decoration: BoxDecoration(
                  color: kSurfaceDark.withValues(alpha: 0.6),
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(color: kGlassBorder),
                ),
                child: Stack(
                  clipBehavior: Clip.none,
                  children: [
                    const Center(
                      child: Icon(LucideIcons.bell, size: 18, color: kTextSecondary),
                    ),
                    if (hasOrders)
                      Positioned(
                        right: 4,
                        top: 4,
                        child: Container(
                          width: 8,
                          height: 8,
                          decoration: const BoxDecoration(
                            color: kNeonCyan,
                            shape: BoxShape.circle,
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
            const SizedBox(width: 10),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(
                color: kSuccess.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(20),
                border: Border.all(color: kSuccess.withValues(alpha: 0.3)),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 7,
                    height: 7,
                    decoration: const BoxDecoration(color: kSuccess, shape: BoxShape.circle),
                  ),
                  const SizedBox(width: 6),
                  Text('在线',
                      style: GoogleFonts.inter(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                        color: kSuccess,
                      )),
                ],
              ),
            ),
          ],
        ),
      ],
    );
  }
}

class _TodaySummary extends StatelessWidget {
  const _TodaySummary();

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();
    final now = DateTime.now();
    final todayOrders = svc.orders.where((o) {
      if (o.status != 'confirmed') return false;
      final created = DateTime.fromMillisecondsSinceEpoch(o.createdAt * 1000);
      return created.year == now.year && created.month == now.month && created.day == now.day;
    }).toList();

    final totalAmount = todayOrders.fold<BigInt>(BigInt.zero, (sum, o) => sum + o.amount);
    final displayAmount = (totalAmount.toDouble() / 1_000_000_000).toStringAsFixed(2);

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(20),
      decoration: glassCardDecoration(),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('今日收款', style: sectionLabel()),
                const SizedBox(height: 6),
                Text('$displayAmount USDC',
                    style: GoogleFonts.jetBrainsMono(
                      fontSize: 22,
                      fontWeight: FontWeight.w700,
                      color: kNeonCyan,
                    )),
              ],
            ),
          ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Text('累计订单', style: sectionLabel()),
                const SizedBox(height: 6),
                Text('${todayOrders.length} 笔',
                    style: GoogleFonts.inter(
                      fontSize: 20,
                      fontWeight: FontWeight.w700,
                      color: kTextPrimary,
                    )),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _QuickActions extends StatelessWidget {
  final VoidCallback onGenerateQr;
  const _QuickActions({required this.onGenerateQr});

  @override
  Widget build(BuildContext context) {
    final svc = context.read<MerchantService>();
    return Column(
      children: [
        Row(
          children: [
            Expanded(
              child: _ActionCard(
                icon: LucideIcons.qrCode,
                label: '生成收款码',
                gradientColors: const [kNeonCyan, kNeonCyanDim],
                onTap: onGenerateQr,
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: _ActionCard(
                icon: LucideIcons.layers,
                label: '通道管理',
                gradientColors: const [kPurple, kPurpleDim],
                onTap: () => openChannelScreen(context),
              ),
            ),
          ],
        ),
        const SizedBox(height: 10),
        Row(
          children: [
            Expanded(
              child: _ActionCard(
                icon: LucideIcons.plusCircle,
                label: '创建通道',
                gradientColors: const [kSuccess, Color(0xFF00C853)],
                onTap: () async {
                  final prefs = await SharedPreferences.getInstance();
                  final registryUrl = prefs.getString('hub_registry_url') ?? 'http://localhost:3004';
                  if (context.mounted) {
                    Navigator.of(context).push(
                      PageRouteBuilder(
                        transitionDuration: const Duration(milliseconds: 350),
                        pageBuilder: (_, animation, _) => SlideTransition(
                          position: Tween<Offset>(
                            begin: const Offset(1, 0),
                            end: Offset.zero,
                          ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
                          child: HubSelectionScreen(
                            registryUrl: registryUrl,
                            storagePath: svc.storagePath,
                            mcpDid: svc.did,
                          ),
                        ),
                      ),
                    );
                  }
                },
              ),
            ),
            const SizedBox(width: 10),
            Expanded(child: Container()), // spacer
          ],
        ),
      ],
    );
  }
}

class _ActionCard extends StatelessWidget {
  final IconData icon;
  final String label;
  final List<Color> gradientColors;
  final VoidCallback onTap;

  const _ActionCard({
    required this.icon,
    required this.label,
    required this.gradientColors,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(
          color: kSurfaceDark.withValues(alpha: 0.6),
          borderRadius: BorderRadius.circular(14),
          border: Border.all(color: kGlassBorder),
        ),
        child: Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(10),
                gradient: LinearGradient(
                  colors: gradientColors,
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
              ),
              child: Icon(icon, size: 18, color: kBackground),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(label,
                  style: GoogleFonts.inter(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: kTextPrimary,
                  )),
            ),
            const Icon(LucideIcons.chevronRight, size: 16, color: kTextSecondary),
          ],
        ),
      ),
    );
  }
}

class _RecentOrders extends StatelessWidget {
  const _RecentOrders();

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();
    final recent = svc.orders.take(5).toList();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(LucideIcons.receipt, size: 16, color: kNeonCyan.withValues(alpha: 0.8)),
            const SizedBox(width: 8),
            Text('最近收款', style: sectionLabel()),
          ],
        ),
        const SizedBox(height: 12),
        if (recent.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 32),
            child: Center(
              child: Column(
                children: [
                  Icon(LucideIcons.inbox, size: 36, color: kTextTertiary),
                  const SizedBox(height: 8),
                  Text('暂无收款记录', style: GoogleFonts.inter(fontSize: 13, color: kTextTertiary)),
                ],
              ),
            ),
          )
        else
          ...recent.map((order) => Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: OrderCard(
              order: order,
              onTap: () => openPaymentDetail(context, order),
            ),
          )),
      ],
    );
  }
}
