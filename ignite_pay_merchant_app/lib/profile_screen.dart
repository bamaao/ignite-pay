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
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/services/channel_service.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';
import 'package:provider/provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openProfile(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const ProfileScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Profile Screen
// ---------------------------------------------------------------------------
class ProfileScreen extends StatefulWidget {
  const ProfileScreen({super.key});

  @override
  State<ProfileScreen> createState() => _ProfileScreenState();
}

class _ProfileScreenState extends State<ProfileScreen> {
  bool _isLoading = true;
  String? _error;
  String _displayName = '';
  String _network = 'devnet';
  int _channelCount = 0;
  int _totalBalance = 0;
  int _confirmedOrders = 0;
  bool _isConnected = false;
  final _nameController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _loadProfile();
  }

  @override
  void dispose() {
    _nameController.dispose();
    super.dispose();
  }

  Future<void> _loadProfile() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      if (!mounted) return;
      final channelSvc = context.read<ChannelService>();
      await channelSvc.refreshChannels();
      if (!mounted) return;
      final merchantSvc = context.read<MerchantService>();
      final pushSvc = context.read<MerchantPushService>();

      final confirmedCount = merchantSvc.orders
          .where((o) => o.status == 'confirmed')
          .length;
      final totalBal = channelSvc.channels.fold<int>(
        0, (sum, c) => sum + c.providerBalance.toInt(),
      );

      if (mounted) {
        setState(() {
          _displayName = prefs.getString('display_name') ?? '';
          _network = prefs.getString('network') ?? 'devnet';
          _channelCount = channelSvc.channels.length;
          _totalBalance = totalBal;
          _confirmedOrders = confirmedCount;
          _isConnected = pushSvc.isConnected;
          _nameController.text = _displayName;
          _isLoading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = e.toString();
          _isLoading = false;
        });
      }
    }
  }

  Future<void> _saveDisplayName(String name) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('display_name', name);
  }

  void _exportDidDoc() {
    final svc = context.read<MerchantService>();
    final didDoc = svc.didDocJson;
    if (didDoc.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          backgroundColor: kAmber,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('DID 文档尚未生成',
              style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          duration: const Duration(seconds: 2),
        ),
      );
      return;
    }
    Clipboard.setData(ClipboardData(text: didDoc));
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        backgroundColor: kSuccess,
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
        margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
        content: Text('DID 文档已复制',
            style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        duration: const Duration(seconds: 2),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();
    final did = svc.did;

    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(height: 12),
              const PageHeader(title: '商户资料', subtitle: '身份与账户设置'),
              const SizedBox(height: 24),

              if (_isLoading)
                const Center(child: Padding(
                  padding: EdgeInsets.symmetric(vertical: 60),
                  child: CircularProgressIndicator(color: kNeonCyan),
                ))
              else if (_error != null)
                _buildError()
              else ...[
                // Avatar
                Center(
                  child: Container(
                    width: 80,
                    height: 80,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      gradient: const LinearGradient(
                        colors: [kPurple, kPurpleDim],
                        begin: Alignment.topLeft,
                        end: Alignment.bottomRight,
                      ),
                      border: Border.all(color: kPurple.withValues(alpha: 0.3), width: 2),
                    ),
                    child: Center(
                      child: Text(
                        did.length >= 2 ? did.substring(0, 2).toUpperCase() : 'M',
                        style: GoogleFonts.jetBrainsMono(
                          fontSize: 24,
                          fontWeight: FontWeight.w700,
                          color: kTextPrimary,
                        ),
                      ),
                    ),
                  ),
                ),
                const SizedBox(height: 20),

                // DID display row
                Container(
                  padding: const EdgeInsets.all(14),
                  decoration: glassDecoration(),
                  child: Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text('DID', style: sectionLabel()),
                            const SizedBox(height: 2),
                            Text(
                              did.isEmpty ? '未生成' : did,
                              style: GoogleFonts.jetBrainsMono(
                                fontSize: 11,
                                color: did.isEmpty ? kTextTertiary : kTextSecondary,
                              ),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ],
                        ),
                      ),
                      if (did.isNotEmpty) ...[
                        const SizedBox(width: 8),
                        GestureDetector(
                          onTap: () {
                            Clipboard.setData(ClipboardData(text: did));
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                backgroundColor: kSuccess,
                                behavior: SnackBarBehavior.floating,
                                shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                                margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                                content: Text('DID 已复制',
                                    style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
                                duration: const Duration(seconds: 2),
                              ),
                            );
                          },
                          child: Icon(LucideIcons.copy, size: 16, color: kTextSecondary),
                        ),
                      ],
                    ],
                  ),
                ),
                const SizedBox(height: 16),

                // Editable display name
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('商户名称', style: sectionLabel()),
                    const SizedBox(height: 4),
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      decoration: BoxDecoration(
                        color: kSurfaceMid,
                        borderRadius: BorderRadius.circular(8),
                        border: Border.all(color: kBorder),
                      ),
                      child: TextField(
                        controller: _nameController,
                        style: GoogleFonts.inter(fontSize: 13, color: kTextPrimary),
                        decoration: InputDecoration(
                          border: InputBorder.none,
                          hintText: '输入商户名称',
                          hintStyle: GoogleFonts.inter(fontSize: 13, color: kTextTertiary),
                          isDense: true,
                          contentPadding: const EdgeInsets.symmetric(vertical: 10),
                        ),
                        onSubmitted: (v) {
                          setState(() => _displayName = v);
                          _saveDisplayName(v);
                        },
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 24),

                // Network info
                const SectionLabel(text: '网络信息'),
                const SizedBox(height: 8),
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                  decoration: glassDecoration(),
                  child: Row(
                    children: [
                      Icon(LucideIcons.globe, size: 16,
                          color: _network == 'mainnet-beta' ? kSuccess : kNeonCyan),
                      const SizedBox(width: 10),
                      Text(
                        _network == 'mainnet-beta' ? 'Mainnet' : 'Devnet',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: _network == 'mainnet-beta' ? kSuccess : kNeonCyan,
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 24),

                // Connection status
                const SectionLabel(text: '连接状态'),
                const SizedBox(height: 8),
                Container(
                  padding: const EdgeInsets.all(14),
                  decoration: glassDecoration(),
                  child: Row(
                    children: [
                      Container(
                        width: 9,
                        height: 9,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: _isConnected ? kSuccess : kDanger,
                        ),
                      ),
                      const SizedBox(width: 10),
                      Text(
                        _isConnected ? '已连接' : '未连接',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w500,
                          color: _isConnected ? kSuccess : kDanger,
                        ),
                      ),
                      const Spacer(),
                      Text(
                        svc.hubEndpoint.isEmpty ? '未配置 Hub' : svc.hubEndpoint,
                        style: GoogleFonts.jetBrainsMono(
                          fontSize: 10,
                          color: kTextTertiary,
                        ),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 24),

                // Statistics
                const SectionLabel(text: '统计数据'),
                const SizedBox(height: 8),
                Row(
                  children: [
                    Expanded(
                      child: _StatCard(
                        label: '通道数',
                        value: _channelCount.toString(),
                        icon: LucideIcons.layers,
                        color: kCyan,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: _StatCard(
                        label: '余额',
                        value: '${(_totalBalance / 1e9).toStringAsFixed(2)} SOL',
                        icon: LucideIcons.wallet,
                        color: kAmber,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: _StatCard(
                        label: '已确认订单',
                        value: _confirmedOrders.toString(),
                        icon: LucideIcons.receipt,
                        color: kSuccess,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 24),

                // Export DID Document
                SizedBox(
                  width: double.infinity,
                  child: OutlinedButton.icon(
                    onPressed: _exportDidDoc,
                    icon: const Icon(LucideIcons.fileDown, size: 16),
                    label: const Text('导出 DID 文档'),
                    style: OutlinedButton.styleFrom(
                      foregroundColor: kNeonCyan,
                      side: const BorderSide(color: kNeonCyan),
                      padding: const EdgeInsets.symmetric(vertical: 12),
                      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                    ),
                  ),
                ),
                const SizedBox(height: 40),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildError() {
    return Center(
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 60),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(LucideIcons.alertCircle, size: 40, color: kDanger),
            const SizedBox(height: 14),
            Text(
              '加载失败',
              style: GoogleFonts.inter(
                fontSize: 15,
                fontWeight: FontWeight.w600,
                color: kTextSecondary,
              ),
            ),
            const SizedBox(height: 8),
            Text(
              _error ?? '',
              style: GoogleFonts.inter(fontSize: 12, color: kTextTertiary),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 16),
            GestureDetector(
              onTap: () => setState(() { _isLoading = true; _error = null; _loadProfile(); }),
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 10),
                decoration: BoxDecoration(
                  color: kNeonCyan.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(20),
                  border: Border.all(color: kNeonCyan.withValues(alpha: 0.25)),
                ),
                child: Text('重试',
                    style: GoogleFonts.inter(fontSize: 12, fontWeight: FontWeight.w600, color: kNeonCyan)),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Stat Card
// ---------------------------------------------------------------------------
class _StatCard extends StatelessWidget {
  final String label;
  final String value;
  final IconData icon;
  final Color color;

  const _StatCard({
    required this.label,
    required this.value,
    required this.icon,
    required this.color,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: kSurfaceMid.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: kGlassBorder),
        gradient: LinearGradient(
          colors: [
            kSurfaceMid.withValues(alpha: 0.7),
            kSurfaceDark.withValues(alpha: 0.5),
          ],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
      ),
      child: Column(
        children: [
          Icon(icon, size: 18, color: color),
          const SizedBox(height: 8),
          Text(
            value,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 14,
              fontWeight: FontWeight.w600,
              color: kTextPrimary,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 4),
          Text(
            label,
            style: GoogleFonts.inter(
              fontSize: 10,
              color: kTextSecondary,
            ),
          ),
        ],
      ),
    );
  }
}
