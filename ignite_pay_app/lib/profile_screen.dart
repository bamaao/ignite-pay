import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/channel_service.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
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
  int _merchantCount = 0;
  bool _hasActiveSessionKey = false;
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
      final didService = DidcommService();

      final channelSvc = ChannelService();
      await channelSvc.refreshChannels(didService.storagePath);

      final merchantDids = prefs.getStringList('known_merchant_dids') ?? [];
      final sessionKeyService = SessionKeyService();
      await sessionKeyService.initialize();

      if (mounted) {
        setState(() {
          _displayName = prefs.getString('display_name') ?? '';
          _network = prefs.getString('network') ?? 'devnet';
          _channelCount = channelSvc.channels.length;
          _totalBalance = channelSvc.totalBalance;
          _merchantCount = merchantDids.length;
          _hasActiveSessionKey = sessionKeyService.activeSessionKey != null;
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
    final didDoc = DidcommService().didDocJson;
    Clipboard.setData(ClipboardData(text: didDoc));
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        backgroundColor: kSuccess,
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
        margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
        content: Text('DID Document copied to clipboard',
            style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        duration: const Duration(seconds: 2),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final did = DidcommService().did;

    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(height: 12),
              const PageHeader(title: 'Profile', subtitle: 'Identity & account settings'),
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
                        did.length >= 2 ? did.substring(0, 2).toUpperCase() : '??',
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
                        child: Text(
                          did,
                          style: GoogleFonts.jetBrainsMono(
                            fontSize: 11,
                            color: kTextSecondary,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
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
                              content: Text('DID copied',
                                  style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
                              duration: const Duration(seconds: 2),
                            ),
                          );
                        },
                        child: Icon(LucideIcons.copy, size: 16, color: kTextSecondary),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 16),

                // Editable display name
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('DISPLAY NAME', style: sectionLabel()),
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
                          hintText: 'Enter display name',
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
                const SectionLabel(text: 'NETWORK INFO'),
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

                // Device status
                const SectionLabel(text: 'DEVICE STATUS'),
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
                          color: DidcommService().isConnected ? kSuccess : kDanger,
                        ),
                      ),
                      const SizedBox(width: 10),
                      Text(
                        DidcommService().isConnected ? 'Connected' : 'Disconnected',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w500,
                          color: DidcommService().isConnected ? kSuccess : kDanger,
                        ),
                      ),
                      const Spacer(),
                      _StatusBadge(
                        label: _hasActiveSessionKey ? 'Session Key Active' : 'No Session Key',
                        color: _hasActiveSessionKey ? kSuccess : kAmber,
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 24),

                // Statistics
                const SectionLabel(text: 'STATISTICS'),
                const SizedBox(height: 8),
                Row(
                  children: [
                    Expanded(
                      child: _StatCard(
                        label: 'Channels',
                        value: _channelCount.toString(),
                        icon: LucideIcons.layers,
                        color: kCyan,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: _StatCard(
                        label: 'Balance',
                        value: '${(_totalBalance / 1e9).toStringAsFixed(2)} SOL',
                        icon: LucideIcons.wallet,
                        color: kAmber,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: _StatCard(
                        label: 'Merchants',
                        value: _merchantCount.toString(),
                        icon: LucideIcons.store,
                        color: kPurple,
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
                    label: const Text('Export DID Document'),
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
              'Failed to load profile',
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
                child: Text('Retry',
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

// ---------------------------------------------------------------------------
// Status Badge
// ---------------------------------------------------------------------------
class _StatusBadge extends StatelessWidget {
  final String label;
  final Color color;

  const _StatusBadge({required this.label, required this.color});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: color.withValues(alpha: 0.3)),
      ),
      child: Text(
        label,
        style: GoogleFonts.inter(
          fontSize: 10,
          fontWeight: FontWeight.w600,
          color: color,
        ),
      ),
    );
  }
}
