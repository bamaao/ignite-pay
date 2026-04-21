import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/channel_service.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:provider/provider.dart';

void openChannelDetail(BuildContext context, ChannelInfo channel) {
  Navigator.of(context).push(PageRouteBuilder(
    pageBuilder: (_, __, ___) => ChannelDetailScreen(channel: channel),
    transitionsBuilder: (_, anim, __, child) =>
        SlideTransition(position: Tween(begin: const Offset(1, 0), end: Offset.zero).animate(anim), child: child),
  ));
}

class ChannelDetailScreen extends StatelessWidget {
  final ChannelInfo channel;
  const ChannelDetailScreen({super.key, required this.channel});

  @override
  Widget build(BuildContext context) {
    final balance = (channel.providerBalance.toDouble() / 1_000_000_000).toStringAsFixed(2);
    final deposited = (channel.totalDeposited.toDouble() / 1_000_000_000).toStringAsFixed(2);

    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const PageHeader(title: '通道详情'),
                const SizedBox(height: 16),
                // Channel info card
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(16),
                  decoration: glassCardDecoration(),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      _InfoRow(label: '通道 ID', value: channel.channelId, mono: true),
                      const SizedBox(height: 12),
                      _InfoRow(label: '状态', value: channel.status),
                      const SizedBox(height: 12),
                      _InfoRow(label: '序列号', value: '${channel.sequence.toRadixString(10)}'),
                      const SizedBox(height: 12),
                      _InfoRow(label: '叶子数', value: '${channel.leafCount}'),
                      const SizedBox(height: 12),
                      _InfoRow(label: '余额', value: '$balance USDC', mono: true),
                      const SizedBox(height: 12),
                      _InfoRow(label: '总存入', value: '$deposited USDC', mono: true),
                    ],
                  ),
                ),
                const SizedBox(height: 24),
                // Close channel button
                GestureDetector(
                  onTap: () => _closeChannel(context),
                  child: Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    decoration: BoxDecoration(
                      color: kDanger.withValues(alpha: 0.1),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(color: kDanger.withValues(alpha: 0.3)),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        const Icon(LucideIcons.xCircle, size: 18, color: kDanger),
                        const SizedBox(width: 8),
                        Text('关闭通道',
                            style: GoogleFonts.inter(
                              fontSize: 14, fontWeight: FontWeight.w600, color: kDanger,
                            )),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 12),
                // Settle button
                GestureDetector(
                  onTap: () => _settleChannel(context),
                  child: Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    decoration: BoxDecoration(
                      gradient: const LinearGradient(colors: [kNeonCyan, kNeonCyanDim]),
                      borderRadius: BorderRadius.circular(12),
                      boxShadow: [BoxShadow(color: kNeonCyan.withValues(alpha: 0.25), blurRadius: 16, spreadRadius: 2)],
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        const Icon(LucideIcons.arrowDownToLine, size: 18, color: kBackground),
                        const SizedBox(width: 8),
                        Text('结算 (Claim + Finalize)',
                            style: GoogleFonts.inter(
                              fontSize: 14, fontWeight: FontWeight.w700, color: kBackground,
                            )),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _closeChannel(BuildContext context) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (_) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        title: Text('确认关闭通道?', style: GoogleFonts.inter(color: kTextPrimary)),
        content: Text('通道 ${channel.channelId.substring(0, 8)}... 将被协作关闭。',
            style: GoogleFonts.inter(color: kTextSecondary)),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text('取消', style: GoogleFonts.inter(color: kTextSecondary))),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text('确认', style: GoogleFonts.inter(color: kDanger))),
        ],
      ),
    );
    if (confirmed != true || !context.mounted) return;

    try {
      final hub = context.read<MerchantService>().hubEndpoint;
      final svc = context.read<ChannelService>();
      final result = await svc.closeChannel(channel.channelId, hub);
      await svc.refreshChannels();
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          backgroundColor: kSuccess,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text(result, style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        ));
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          backgroundColor: kDanger,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('关闭失败: $e', style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        ));
      }
    }
  }

  Future<void> _settleChannel(BuildContext context) async {
    try {
      final hub = context.read<MerchantService>().hubEndpoint;
      final svc = context.read<ChannelService>();
      // Claim all leaves then finalize
      await svc.claimLeaf(channel.channelId, hub, 0, channel.providerBalance);
      final result = await svc.finalize(channel.channelId, hub);
      await svc.refreshChannels();
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          backgroundColor: kSuccess,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text(result, style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        ));
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          backgroundColor: kDanger,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('结算失败: $e', style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        ));
      }
    }
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  final bool mono;

  const _InfoRow({required this.label, required this.value, this.mono = false});

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
          child: Text(
            value,
            style: mono ? monoValue(12) : GoogleFonts.inter(fontSize: 13, color: kTextPrimary),
            overflow: TextOverflow.ellipsis,
          ),
        ),
      ],
    );
  }
}
