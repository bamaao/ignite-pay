import 'dart:async';
import 'dart:math';
import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/channel_service.dart';
import 'package:ignite_pay_app/services/app_log_service.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as rust;

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openChannelTopology(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const ChannelTopologyScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Channel Topology Screen
// ---------------------------------------------------------------------------
class ChannelTopologyScreen extends StatefulWidget {
  const ChannelTopologyScreen({super.key});

  @override
  State<ChannelTopologyScreen> createState() => _ChannelTopologyScreenState();
}

class _ChannelTopologyScreenState extends State<ChannelTopologyScreen> {
  bool _isLoading = true;
  String? _error;
  final ChannelService _channelSvc = ChannelService();
  StreamSubscription<MbDepositResult>? _mbDepositSub;

  @override
  void initState() {
    super.initState();
    _loadChannels();
  }

  @override
  void dispose() {
    _mbDepositSub?.cancel();
    super.dispose();
  }

  Future<void> _loadChannels() async {
    try {
      await _channelSvc.refreshChannels(DidcommService().storagePath);
      if (mounted) setState(() { _isLoading = false; _error = null; });
    } catch (e) {
      if (mounted) setState(() { _isLoading = false; _error = e.toString(); });
    }
  }

  void _showDepositSheet() {
    final amountController = TextEditingController();
    bool isSubmitting = false;
    String selectedToken = 'USDC'; // Default to USDC

    const tokenOptions = ['USDC', 'USDT', 'SOL'];
    const tokenDecimals = {'USDC': 6, 'USDT': 6, 'SOL': 9};

    showModalBottomSheet(
      context: context,
      backgroundColor: kSurfaceDark,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
      ),
      builder: (sheetContext) {
        return StatefulBuilder(
          builder: (context, setSheetState) {
            return SafeArea(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(20, 16, 20, 20),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Center(
                      child: Container(
                        width: 40,
                        height: 4,
                        decoration: BoxDecoration(
                          color: kTextTertiary,
                          borderRadius: BorderRadius.circular(2),
                        ),
                      ),
                    ),
                    const SizedBox(height: 16),
                    Text(
                      'Deposit to MB Vault',
                      style: GoogleFonts.inter(
                        fontSize: 18,
                        fontWeight: FontWeight.w700,
                        color: kTextPrimary,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Text(
                      'Deposit SOL into the MagicBlock shared vault for instant payments.',
                      style: GoogleFonts.inter(fontSize: 12, color: kTextSecondary),
                    ),
                    const SizedBox(height: 16),
                    // Token selector
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
                      decoration: BoxDecoration(
                        color: kBackground,
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(color: kGlassBorder),
                      ),
                      child: DropdownButtonHideUnderline(
                        child: DropdownButton<String>(
                          value: selectedToken,
                          isExpanded: true,
                          dropdownColor: kSurfaceDark,
                          style: GoogleFonts.inter(
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                            color: kTextPrimary,
                          ),
                          items: tokenOptions.map((t) => DropdownMenuItem(
                            value: t,
                            child: Text(t),
                          )).toList(),
                          onChanged: (v) {
                            if (v != null) setSheetState(() => selectedToken = v);
                          },
                        ),
                      ),
                    ),
                    const SizedBox(height: 12),

                    TextField(
                      controller: amountController,
                      keyboardType: const TextInputType.numberWithOptions(decimal: true),
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 16,
                        color: kTextPrimary,
                      ),
                      decoration: InputDecoration(
                        labelText: 'Amount',
                        labelStyle: GoogleFonts.inter(color: kTextSecondary),
                        suffixText: selectedToken,
                        suffixStyle: GoogleFonts.inter(color: kTextTertiary, fontSize: 12),
                        enabledBorder: OutlineInputBorder(
                          borderRadius: BorderRadius.circular(12),
                          borderSide: const BorderSide(color: kGlassBorder),
                        ),
                        focusedBorder: OutlineInputBorder(
                          borderRadius: BorderRadius.circular(12),
                          borderSide: const BorderSide(color: kNeonCyan),
                        ),
                      ),
                    ),
                    const SizedBox(height: 16),
                    SizedBox(
                      width: double.infinity,
                      child: ElevatedButton(
                        onPressed: isSubmitting
                            ? null
                            : () async {
                                final amountStr = amountController.text.trim();
                                final amountVal = double.tryParse(amountStr);
                                if (amountVal == null || amountVal <= 0) {
                                  ScaffoldMessenger.of(context).showSnackBar(
                                    SnackBar(
                                      backgroundColor: kDanger,
                                      behavior: SnackBarBehavior.floating,
                                      shape: RoundedRectangleBorder(
                                          borderRadius: BorderRadius.circular(10)),
                                      content: Text('Enter a valid amount',
                                          style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
                                    ),
                                  );
                                  return;
                                }
                                final decimals = tokenDecimals[selectedToken] ?? 9;
                                // Use correct power-of-10 for decimals
                                final baseAmount = (amountVal * (pow(10, decimals) as double)).round();
                                if (baseAmount <= 0) {
                                  return;
                                }

                                setSheetState(() => isSubmitting = true);

                                try {
                                  // Listen for the response
                                  _mbDepositSub?.cancel();
                                  _mbDepositSub = DidcommService().mbDepositResults.listen((result) {
                                    if (!mounted) return;
                                    Navigator.of(context).maybePop();
                                    final snackBar = SnackBar(
                                      backgroundColor: result.success ? kSuccess : kDanger,
                                      behavior: SnackBarBehavior.floating,
                                      shape: RoundedRectangleBorder(
                                          borderRadius: BorderRadius.circular(10)),
                                      margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                                      content: Text(
                                        result.success
                                            ? 'Deposited ${(result.depositAmount / 1e9).toStringAsFixed(4)} SOL'
                                                '${result.totalDeposited != null ? ' (total: ${(result.totalDeposited! / 1e9).toStringAsFixed(4)} SOL)' : ''}'
                                            : 'Deposit failed: ${result.error ?? "unknown error"}',
                                        style: GoogleFonts.inter(fontWeight: FontWeight.w600),
                                      ),
                                      duration: const Duration(seconds: 3),
                                    );
                                    ScaffoldMessenger.of(context).showSnackBar(snackBar);
                                    _loadChannels();
                                  });

                                  await rust.sendMbDepositRequest(
                                    storagePath: DidcommService().storagePath,
                                    amount: BigInt.from(baseAmount),
                                    token: selectedToken,
                                  );
                                  AppLogService().info('MB', 'Deposit request sent: $amountVal $selectedToken');
                                } catch (e) {
                                  if (mounted) {
                                    setSheetState(() => isSubmitting = false);
                                    ScaffoldMessenger.of(context).showSnackBar(
                                      SnackBar(
                                        backgroundColor: kDanger,
                                        behavior: SnackBarBehavior.floating,
                                        shape: RoundedRectangleBorder(
                                            borderRadius: BorderRadius.circular(10)),
                                        content: Text('Failed to send deposit: $e',
                                            style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
                                      ),
                                    );
                                  }
                                }
                              },
                        style: ElevatedButton.styleFrom(
                          backgroundColor: kNeonCyan,
                          foregroundColor: kBackground,
                          padding: const EdgeInsets.symmetric(vertical: 14),
                          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                          textStyle: GoogleFonts.inter(fontSize: 14, fontWeight: FontWeight.w700),
                        ),
                        child: isSubmitting
                            ? const SizedBox(
                                width: 20,
                                height: 20,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                  color: kBackground,
                                ),
                              )
                            : const Text('Confirm Deposit'),
                      ),
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final did = DidcommService().did;
    final channels = _channelSvc.channels;
    final totalBalance = _channelSvc.totalBalance;
    final openCount = channels.where((c) => c.status.toLowerCase() == 'open').length;
    final closedCount = channels.length - openCount;

    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
              child: const PageHeader(
                title: 'Channel Topology',
                subtitle: 'State channel network',
              ),
            ),
            const SizedBox(height: 20),

            if (_isLoading)
              const Expanded(child: Center(child: CircularProgressIndicator(color: kNeonCyan)))
            else if (_error != null)
              Expanded(child: _buildError())
            else if (channels.isEmpty)
              Expanded(child: _buildEmptyState())
            else ...[
              // Balance summary
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 20),
                child: _BalanceSummaryCard(
                  totalBalance: totalBalance,
                  openCount: openCount,
                  closedCount: closedCount,
                ),
              ),
              const SizedBox(height: 12),

              // MB Vault deposit button
              if (DidcommService().isConnected)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  child: SizedBox(
                    width: double.infinity,
                    child: OutlinedButton.icon(
                      onPressed: _showDepositSheet,
                      icon: const Icon(LucideIcons.wallet, size: 16),
                      label: const Text('Deposit to MB Vault'),
                      style: OutlinedButton.styleFrom(
                        foregroundColor: kNeonCyan,
                        side: const BorderSide(color: kNeonCyan),
                        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                        textStyle: GoogleFonts.inter(fontSize: 13, fontWeight: FontWeight.w600),
                      ),
                    ),
                  ),
                ),
              const SizedBox(height: 16),

              // Your node card
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 20),
                child: Container(
                  padding: const EdgeInsets.all(14),
                  decoration: glassDecoration(),
                  child: Row(
                    children: [
                      Icon(LucideIcons.cpu, size: 16, color: kNeonCyan),
                      const SizedBox(width: 10),
                      Text(
                        'Your Node',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: kTextPrimary,
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Text(
                          _shortenDid(did),
                          style: GoogleFonts.jetBrainsMono(
                            fontSize: 11,
                            color: kTextSecondary,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      const SizedBox(width: 8),
                      _PulsatingDot(connected: DidcommService().isConnected),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 20),

              // Channel list
              Expanded(
                child: RefreshIndicator(
                  color: kNeonCyan,
                  backgroundColor: kSurfaceDark,
                  onRefresh: _loadChannels,
                  child: ListView.separated(
                    padding: const EdgeInsets.symmetric(horizontal: 20),
                    itemCount: channels.length,
                    separatorBuilder: (_, __) => const SizedBox(height: 8),
                    itemBuilder: (context, index) {
                      return _ChannelCard(
                        channel: channels[index],
                        onClose: () => _closeChannel(channels[index]),
                        onSettle: () => _settleChannel(channels[index]),
                      );
                    },
                  ),
                ),
              ),
              const SizedBox(height: 20),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildError() {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(LucideIcons.alertCircle, size: 40, color: kDanger),
          const SizedBox(height: 14),
          Text(
            'Failed to load channels',
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
            onTap: () => setState(() { _isLoading = true; _error = null; _loadChannels(); }),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 10),
              decoration: BoxDecoration(
                color: kNeonCyan.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(20),
                border: Border.all(color: kNeonCyan.withValues(alpha: 0.25)),
              ),
              child: Text('Retry',
                  style: GoogleFonts.inter(fontSize: 12, fontWeight: FontWeight.w600, color: kNeonCyan)),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildEmptyState() {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(LucideIcons.layers, size: 40, color: kTextTertiary),
          const SizedBox(height: 14),
          Text(
            'No state channels',
            style: GoogleFonts.inter(
              fontSize: 15,
              fontWeight: FontWeight.w600,
              color: kTextSecondary,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            'Create a channel to get started',
            style: GoogleFonts.inter(fontSize: 12, color: kTextTertiary),
          ),
        ],
      ),
    );
  }

  Future<void> _closeChannel(LocalChannelInfo channel) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
        title: Text('Close Channel?',
            style: GoogleFonts.inter(fontSize: 16, fontWeight: FontWeight.w700, color: kTextPrimary)),
        content: Text(
          'This will close channel ${_shortenDid(channel.channelId)}. Remaining balance will be settled.',
          style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: Text('Cancel', style: GoogleFonts.inter(color: kTextSecondary)),
          ),
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(true),
            child: Text('Close', style: GoogleFonts.inter(color: kDanger)),
          ),
        ],
      ),
    );

    if (confirmed != true || !mounted) return;

    try {
      await rust.closeChannel(
        storagePath: DidcommService().storagePath,
        channelId: channel.channelId,
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Channel closed',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 2),
          ),
        );
      }
      _loadChannels();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kDanger,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Failed to close channel: $e',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 3),
          ),
        );
      }
    }
  }

  Future<void> _settleChannel(LocalChannelInfo channel) async {
    try {
      await rust.settleChannel(
        storagePath: DidcommService().storagePath,
        channelId: channel.channelId,
        hubEndpoint: channel.hubEndpoint,
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Channel settled',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 2),
          ),
        );
      }
      _loadChannels();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kDanger,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Failed to settle channel: $e',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 3),
          ),
        );
      }
    }
  }

  String _shortenDid(String did) {
    if (did.length > 24) return '${did.substring(0, 16)}...${did.substring(did.length - 6)}';
    return did;
  }
}

// ---------------------------------------------------------------------------
// Balance Summary Card
// ---------------------------------------------------------------------------
class _BalanceSummaryCard extends StatelessWidget {
  final int totalBalance;
  final int openCount;
  final int closedCount;

  const _BalanceSummaryCard({
    required this.totalBalance,
    required this.openCount,
    required this.closedCount,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: kSurfaceMid.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(16),
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
          Text(
            '${(totalBalance / 1e9).toStringAsFixed(4)} SOL',
            style: GoogleFonts.jetBrainsMono(
              fontSize: 28,
              fontWeight: FontWeight.w700,
              color: kTextPrimary,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            'TOTAL BALANCE',
            style: GoogleFonts.inter(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: kTextSecondary,
              letterSpacing: 1.2,
            ),
          ),
          const SizedBox(height: 12),
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              _countChip('$openCount open', kSuccess),
              const SizedBox(width: 12),
              _countChip('$closedCount closed', kDanger),
            ],
          ),
        ],
      ),
    );
  }

  Widget _countChip(String label, Color color) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: color.withValues(alpha: 0.3)),
      ),
      child: Text(
        label,
        style: GoogleFonts.inter(
          fontSize: 11,
          fontWeight: FontWeight.w600,
          color: color,
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Channel Card
// ---------------------------------------------------------------------------
class _ChannelCard extends StatelessWidget {
  final LocalChannelInfo channel;
  final VoidCallback onClose;
  final VoidCallback onSettle;

  const _ChannelCard({
    required this.channel,
    required this.onClose,
    required this.onSettle,
  });

  bool get _isOpen => channel.status.toLowerCase() == 'open';

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: kSurfaceMid.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(14),
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
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  _shortenDid(channel.hubEndpoint),
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 12,
                    color: kTextPrimary,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(width: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                decoration: BoxDecoration(
                  color: (_isOpen ? kSuccess : kDanger).withValues(alpha: 0.12),
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(
                    color: (_isOpen ? kSuccess : kDanger).withValues(alpha: 0.3),
                  ),
                ),
                child: Text(
                  channel.status,
                  style: GoogleFonts.inter(
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                    color: _isOpen ? kSuccess : kDanger,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          _infoRow('Balance', '${(channel.balance / 1e9).toStringAsFixed(4)} SOL'),
          _infoRow('Deposited', '${(channel.totalDeposited / 1e9).toStringAsFixed(4)} SOL'),
          _infoRow('Sequence', '${channel.sequence}'),
          _infoRow('Tree Depth', '${channel.treeDepth}'),
          const SizedBox(height: 12),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              if (_isOpen)
                OutlinedButton.icon(
                  onPressed: onClose,
                  icon: const Icon(LucideIcons.xCircle, size: 14),
                  label: const Text('Close'),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: kDanger,
                    side: const BorderSide(color: kDanger),
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
                    textStyle: GoogleFonts.inter(fontSize: 11, fontWeight: FontWeight.w600),
                  ),
                ),
              if (!_isOpen) ...[
                OutlinedButton.icon(
                  onPressed: onSettle,
                  icon: const Icon(LucideIcons.arrowDownToLine, size: 14),
                  label: const Text('Settle'),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: kNeonCyan,
                    side: const BorderSide(color: kNeonCyan),
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
                    textStyle: GoogleFonts.inter(fontSize: 11, fontWeight: FontWeight.w600),
                  ),
                ),
              ],
            ],
          ),
        ],
      ),
    );
  }

  Widget _infoRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        children: [
          Text(
            label,
            style: GoogleFonts.inter(fontSize: 11, color: kTextTertiary),
          ),
          const Spacer(),
          Text(
            value,
            style: GoogleFonts.jetBrainsMono(fontSize: 11, color: kTextSecondary),
          ),
        ],
      ),
    );
  }

  String _shortenDid(String did) {
    if (did.length > 30) return '${did.substring(0, 20)}...${did.substring(did.length - 6)}';
    return did;
  }
}

// ---------------------------------------------------------------------------
// Pulsating Dot
// ---------------------------------------------------------------------------
class _PulsatingDot extends StatefulWidget {
  final bool connected;
  const _PulsatingDot({required this.connected});

  @override
  State<_PulsatingDot> createState() => _PulsatingDotState();
}

class _PulsatingDotState extends State<_PulsatingDot>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ctrl;

  @override
  void initState() {
    super.initState();
    _ctrl = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1500),
    )..repeat(reverse: true);
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final color = widget.connected ? kSuccess : kDanger;
    return AnimatedBuilder(
      animation: _ctrl,
      builder: (context, child) {
        return Container(
          width: 9,
          height: 9,
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: color,
            boxShadow: [
              BoxShadow(
                color: color.withValues(alpha: 0.4 + 0.4 * _ctrl.value),
                blurRadius: 6 + 4 * _ctrl.value,
                spreadRadius: 1,
              ),
            ],
          ),
        );
      },
    );
  }
}
