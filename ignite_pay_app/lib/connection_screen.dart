import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/qr_scanner_screen.dart';
import 'package:provider/provider.dart';

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openConnectionManagement(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const ConnectionManagementScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Connection Management Screen
// ---------------------------------------------------------------------------
class ConnectionManagementScreen extends StatefulWidget {
  const ConnectionManagementScreen({super.key});

  @override
  State<ConnectionManagementScreen> createState() =>
      _ConnectionManagementScreenState();
}

class _ConnectionManagementScreenState
    extends State<ConnectionManagementScreen> {
  final _wsUrlController = TextEditingController();
  bool _isConnecting = false;

  @override
  void initState() {
    super.initState();
    final svc = DidcommService();
    _wsUrlController.text = svc.mediatorWsUrl.isNotEmpty
        ? svc.mediatorWsUrl
        : 'wss://relay.ignite.did';
  }

  @override
  void dispose() {
    _wsUrlController.dispose();
    super.dispose();
  }

  Future<void> _connectMediator() async {
    if (_isConnecting) return;
    setState(() => _isConnecting = true);
    try {
      await DidcommService().connectToMediator(_wsUrlController.text.trim());
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape:
                RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Connected to mediator',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
            duration: const Duration(seconds: 2),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        _showErrorDialog('Connection Failed', e.toString());
      }
    } finally {
      if (mounted) setState(() => _isConnecting = false);
    }
  }

  void _showErrorDialog(String title, String message) {
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
        title: Row(
          children: [
            Icon(LucideIcons.alertCircle, size: 20, color: kDanger),
            const SizedBox(width: 10),
            Text(title,
                style: GoogleFonts.inter(
                    fontSize: 16, fontWeight: FontWeight.w600, color: kTextPrimary)),
          ],
        ),
        content: Text(message,
            style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: Text('OK',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600, color: kNeonCyan)),
          ),
        ],
      ),
    );
  }

  Future<void> _disconnectMediator() async {
    await DidcommService().disconnect();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          backgroundColor: kTextSecondary,
          behavior: SnackBarBehavior.floating,
          shape:
              RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('Disconnected',
              style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          duration: const Duration(seconds: 2),
        ),
      );
    }
  }

  Future<void> _scanQr() async {
    final result = await showQrScanner(context);
    if (result != null && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          backgroundColor: kSuccess,
          behavior: SnackBarBehavior.floating,
          shape:
              RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('Paired with $result',
              style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          duration: const Duration(seconds: 2),
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(height: 12),
              const PageHeader(
                title: 'Connections',
                subtitle: 'Mediator & MCP management',
              ),
              const SizedBox(height: 28),
              const SectionLabel(text: 'MEDIATOR'),
              const SizedBox(height: 8),
              _MediatorCard(
                wsUrlController: _wsUrlController,
                isConnecting: _isConnecting,
                onConnect: _connectMediator,
                onDisconnect: _disconnectMediator,
              ),
              const SizedBox(height: 24),
              const SectionLabel(text: 'PUSH CHANNEL'),
              const SizedBox(height: 8),
              const _PushChannelCard(),
              const SizedBox(height: 24),
              const SectionLabel(text: 'PAIRED MCP AGENTS'),
              const SizedBox(height: 8),
              const _McpListCard(),
              const SizedBox(height: 12),
              _AddMcpButton(onTap: _scanQr),
              const SizedBox(height: 40),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Mediator Card
// ---------------------------------------------------------------------------
class _MediatorCard extends StatelessWidget {
  final TextEditingController wsUrlController;
  final bool isConnecting;
  final VoidCallback onConnect;
  final VoidCallback onDisconnect;

  const _MediatorCard({
    required this.wsUrlController,
    required this.isConnecting,
    required this.onConnect,
    required this.onDisconnect,
  });

  @override
  Widget build(BuildContext context) {
    return Consumer<DidcommService>(
      builder: (context, svc, _) {
        final connected = svc.isConnected;
        final statusColor = connected ? kSuccess : kDanger;

        return Container(
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            color: kSurfaceDark,
            borderRadius: BorderRadius.circular(14),
            border: Border.all(color: statusColor.withValues(alpha: 0.2)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Status row
              Row(
                children: [
                  Container(
                    width: 9,
                    height: 9,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: statusColor,
                      boxShadow: connected
                          ? [
                              BoxShadow(
                                color: statusColor.withValues(alpha: 0.5),
                                blurRadius: 6,
                                spreadRadius: 1,
                              ),
                            ]
                          : null,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    connected ? 'Connected' : 'Disconnected',
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                      color: statusColor,
                    ),
                  ),
                  const Spacer(),
                  Icon(
                    LucideIcons.radio,
                    size: 16,
                    color: statusColor.withValues(alpha: 0.6),
                  ),
                ],
              ),
              const SizedBox(height: 16),
              // WS URL input
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                decoration: BoxDecoration(
                  color: kSurfaceMid,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: kBorder),
                ),
                child: TextField(
                  controller: wsUrlController,
                  enabled: !connected && !isConnecting,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 13,
                    color: kTextPrimary,
                  ),
                  decoration: InputDecoration(
                    border: InputBorder.none,
                    hintText: 'wss://relay.ignite.did',
                    hintStyle: GoogleFonts.jetBrainsMono(
                      fontSize: 13,
                      color: kTextTertiary,
                    ),
                    isDense: true,
                    contentPadding: const EdgeInsets.symmetric(vertical: 10),
                  ),
                ),
              ),
              const SizedBox(height: 12),
              // Connect / Disconnect button
              SizedBox(
                width: double.infinity,
                height: 44,
                child: ElevatedButton(
                  onPressed: isConnecting
                      ? null
                      : (connected ? onDisconnect : onConnect),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: connected
                        ? kDanger.withValues(alpha: 0.15)
                        : kNeonCyan.withValues(alpha: 0.15),
                    foregroundColor: connected ? kDanger : kNeonCyan,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(10),
                      side: BorderSide(
                        color: (connected ? kDanger : kNeonCyan)
                            .withValues(alpha: 0.3),
                      ),
                    ),
                  ),
                  child: isConnecting
                      ? SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: kNeonCyan.withValues(alpha: 0.7),
                          ),
                        )
                      : Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            Icon(
                              connected
                                  ? LucideIcons.unplug
                                  : LucideIcons.plug,
                              size: 16,
                            ),
                            const SizedBox(width: 8),
                            Text(
                              connected ? 'Disconnect' : 'Connect',
                              style: GoogleFonts.inter(
                                fontSize: 13,
                                fontWeight: FontWeight.w600,
                              ),
                            ),
                          ],
                        ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

// ---------------------------------------------------------------------------
// Push Channel Card
// ---------------------------------------------------------------------------
class _PushChannelCard extends StatelessWidget {
  const _PushChannelCard();

  @override
  Widget build(BuildContext context) {
    return Consumer<DidcommService>(
      builder: (context, svc, _) {
        // Determine channel based on locale
        final isWs = svc.isConnected; // simplified: WS if connected
        final channelLabel = isWs ? 'WebSocket' : 'FCM';
        final channelIcon = isWs ? LucideIcons.wifi : LucideIcons.bell;

        return SettingsTile(
          icon: channelIcon,
          iconColor: kCyan,
          title: 'Push Channel',
          subtitle: 'How messages reach your phone',
          trailing: Container(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
            decoration: BoxDecoration(
              color: kCyan.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(6),
              border: Border.all(color: kCyan.withValues(alpha: 0.2)),
            ),
            child: Text(
              channelLabel,
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: kCyan,
              ),
            ),
          ),
        );
      },
    );
  }
}

// ---------------------------------------------------------------------------
// MCP List Card
// ---------------------------------------------------------------------------
class _McpListCard extends StatelessWidget {
  const _McpListCard();

  @override
  Widget build(BuildContext context) {
    return Consumer<DidcommService>(
      builder: (context, svc, _) {
        // Show MCP agents from stored paired MCPs
        final mcps = svc.pairedMcps;

        if (mcps.isEmpty) {
          return Container(
            width: double.infinity,
            padding: const EdgeInsets.all(24),
            decoration: glassDecoration(),
            child: Column(
              children: [
                Icon(LucideIcons.scanLine,
                    size: 28, color: kTextTertiary),
                const SizedBox(height: 10),
                Text(
                  'No MCP agents paired yet',
                  style: GoogleFonts.inter(
                    fontSize: 13,
                    color: kTextSecondary,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  'Scan a QR code to pair with an MCP server',
                  style: GoogleFonts.inter(
                    fontSize: 11,
                    color: kTextTertiary,
                  ),
                ),
              ],
            ),
          );
        }

        return Column(
          children: mcps
              .map((mcp) => _McpTile(
                    did: mcp.did,
                    mediatorUrl: mcp.mediatorHttpUrl,
                    pairedAt: mcp.pairedAt,
                  ))
              .toList(),
        );
      },
    );
  }
}

class _McpTile extends StatelessWidget {
  final String did;
  final String mediatorUrl;
  final DateTime pairedAt;
  const _McpTile({required this.did, required this.mediatorUrl, required this.pairedAt});

  void _showDetail(BuildContext context) {
    final short = did.length > 32 ? '${did.substring(0, 32)}...' : did;
    final dateStr = '${pairedAt.year}-${pairedAt.month.toString().padLeft(2, '0')}-${pairedAt.day.toString().padLeft(2, '0')} '
        '${pairedAt.hour.toString().padLeft(2, '0')}:${pairedAt.minute.toString().padLeft(2, '0')}';

    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
        title: Row(
          children: [
            Icon(LucideIcons.bot, size: 20, color: kPurple),
            const SizedBox(width: 10),
            Text('MCP Agent',
                style: GoogleFonts.inter(
                    fontSize: 16, fontWeight: FontWeight.w600, color: kTextPrimary)),
          ],
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('DID', style: GoogleFonts.inter(fontSize: 11, fontWeight: FontWeight.w600, color: kTextTertiary)),
            const SizedBox(height: 2),
            SelectableText(short, style: GoogleFonts.jetBrainsMono(fontSize: 12, color: kTextPrimary)),
            const SizedBox(height: 12),
            Text('Mediator', style: GoogleFonts.inter(fontSize: 11, fontWeight: FontWeight.w600, color: kTextTertiary)),
            const SizedBox(height: 2),
            SelectableText(mediatorUrl, style: GoogleFonts.jetBrainsMono(fontSize: 12, color: kTextPrimary)),
            const SizedBox(height: 12),
            Text('Paired', style: GoogleFonts.inter(fontSize: 11, fontWeight: FontWeight.w600, color: kTextTertiary)),
            const SizedBox(height: 2),
            Text(dateStr, style: GoogleFonts.inter(fontSize: 12, color: kTextSecondary)),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: Text('Close',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600, color: kNeonCyan)),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final short = did.length > 24 ? '${did.substring(0, 24)}...' : did;
    final mediatorShort = mediatorUrl.length > 30 ? '${mediatorUrl.substring(0, 30)}...' : mediatorUrl;

    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: SettingsTile(
        icon: LucideIcons.bot,
        iconColor: kPurple,
        title: 'MCP Agent',
        subtitle: '$short\n$mediatorShort',
        trailing: Icon(LucideIcons.chevronRight,
            size: 18, color: kTextTertiary),
        onTap: () => _showDetail(context),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Add MCP Button
// ---------------------------------------------------------------------------
class _AddMcpButton extends StatelessWidget {
  final VoidCallback onTap;
  const _AddMcpButton({required this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.symmetric(vertical: 14),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: kPurple.withValues(alpha: 0.3),
          ),
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(LucideIcons.plus, size: 18, color: kPurple),
            const SizedBox(width: 8),
            Text(
              'Add MCP via QR Code',
              style: GoogleFonts.inter(
                fontSize: 13,
                fontWeight: FontWeight.w600,
                color: kPurple,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
