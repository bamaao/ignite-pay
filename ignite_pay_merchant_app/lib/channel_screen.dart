import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/channel_service.dart';
import 'package:ignite_pay_merchant/widgets/channel_card.dart';
import 'package:ignite_pay_merchant/channel_detail_screen.dart';
import 'package:provider/provider.dart';

void openChannelScreen(BuildContext context) {
  Navigator.of(context).push(PageRouteBuilder(
    pageBuilder: (_, __, ___) => const ChannelScreen(),
    transitionsBuilder: (_, anim, __, child) =>
        SlideTransition(position: Tween(begin: const Offset(1, 0), end: Offset.zero).animate(anim), child: child),
  ));
}

class ChannelScreen extends StatefulWidget {
  const ChannelScreen({super.key});

  @override
  State<ChannelScreen> createState() => _ChannelScreenState();
}

class _ChannelScreenState extends State<ChannelScreen> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      context.read<ChannelService>().refreshChannels();
    });
  }

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<ChannelService>();
    final totalBalance = svc.channels.fold<BigInt>(BigInt.zero, (sum, c) => sum + c.providerBalance);
    final displayBalance = (totalBalance.toDouble() / 1_000_000_000).toStringAsFixed(2);

    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const PageHeader(title: '通道管理'),
              const SizedBox(height: 16),
              // Summary
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(16),
                decoration: glassCardDecoration(),
                child: Row(
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text('通道总数', style: sectionLabel()),
                          const SizedBox(height: 4),
                          Text('${svc.channels.length}',
                              style: GoogleFonts.inter(fontSize: 20, fontWeight: FontWeight.w700, color: kTextPrimary)),
                        ],
                      ),
                    ),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.end,
                        children: [
                          Text('总余额', style: sectionLabel()),
                          const SizedBox(height: 4),
                          Text('$displayBalance USDC',
                              style: GoogleFonts.jetBrainsMono(fontSize: 16, fontWeight: FontWeight.w700, color: kNeonCyan)),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 16),
              // Channel list
              Expanded(
                child: RefreshIndicator(
                  color: kNeonCyan,
                  backgroundColor: kSurfaceDark,
                  onRefresh: () => svc.refreshChannels(),
                  child: svc.loading && svc.channels.isEmpty
                      ? const Center(child: CircularProgressIndicator(color: kNeonCyan))
                      : svc.channels.isEmpty
                          ? ListView(children: [
                              SizedBox(height: MediaQuery.of(context).size.height * 0.25),
                              Center(
                                child: Column(
                                  children: [
                                    Icon(LucideIcons.layers, size: 36, color: kTextTertiary),
                                    const SizedBox(height: 8),
                                    Text('暂无通道', style: GoogleFonts.inter(fontSize: 13, color: kTextTertiary)),
                                  ],
                                ),
                              ),
                            ])
                          : ListView.builder(
                              itemCount: svc.channels.length,
                              itemBuilder: (_, i) => Padding(
                                padding: const EdgeInsets.only(bottom: 8),
                                child: ChannelCard(
                                  channel: svc.channels[i],
                                  onTap: () => openChannelDetail(context, svc.channels[i]),
                                ),
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
