import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
import 'package:ignite_pay_app/src/rust/api/session.dart' as session;

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openSessionKeys(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const SessionKeysScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Session Keys Screen
// ---------------------------------------------------------------------------
class SessionKeysScreen extends StatefulWidget {
  const SessionKeysScreen({super.key});

  @override
  State<SessionKeysScreen> createState() => _SessionKeysScreenState();
}

class _SessionKeysScreenState extends State<SessionKeysScreen> {
  final SessionKeyService _svc = SessionKeyService();
  bool _isLoading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadKeys();
  }

  Future<void> _loadKeys() async {
    setState(() {
      _isLoading = true;
      _error = null;
    });
    try {
      await _svc.initialize();
      await _svc.loadAllKeys();
    } catch (e) {
      _error = e.toString();
    }
    if (mounted) {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _revokeKey(String pubkey) async {
    try {
      final txSig = await _svc.revokeKey(pubkey);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Revoked on-chain: ${txSig.substring(0, 16)}...',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kDanger,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Revoke failed: $e',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          ),
        );
      }
    }
  }

  Future<void> _deleteKey(String pubkey) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
        title: Text('Delete Local Key?',
            style: GoogleFonts.inter(
                fontSize: 16, fontWeight: FontWeight.w700, color: kTextPrimary)),
        content: Text(
          'This removes the key from local storage only. It does not revoke it on-chain.',
          style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: Text('Cancel', style: GoogleFonts.inter(color: kTextSecondary)),
          ),
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(true),
            child: Text('Delete', style: GoogleFonts.inter(color: kDanger)),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      await _svc.deleteLocalKey(pubkey);
    }
  }

  Future<void> _registerNewKey() async {
    // Use built-in method for now from the management screen
    try {
      await _svc.createWithBuiltInKey(
        spendingLimit: 5000000000, // 5 SOL
        durationSecs: 86400, // 24 hours
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kSuccess,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Session key registered on-chain',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            backgroundColor: kDanger,
            behavior: SnackBarBehavior.floating,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
            margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
            content: Text('Registration failed: $e',
                style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
          ),
        );
      }
    }
  }

  String _shortenPubkey(String pubkey) {
    if (pubkey.length > 16) {
      return '${pubkey.substring(0, 8)}...${pubkey.substring(pubkey.length - 6)}';
    }
    return pubkey;
  }

  String _formatExpiry(int expiresAt) {
    final dt = DateTime.fromMillisecondsSinceEpoch(expiresAt * 1000);
    final now = DateTime.now();
    final diff = dt.difference(now);
    if (diff.isNegative) return 'Expired';
    if (diff.inHours < 24) return '${diff.inHours}h ${diff.inMinutes % 60}m left';
    return '${diff.inDays}d ${diff.inHours % 24}h left';
  }

  String _formatLimit(BigInt lamports) {
    final sol = lamports.toDouble() / 1000000000.0;
    return '${sol.toStringAsFixed(sol.truncateToDouble() == sol ? 0 : 2)} SOL';
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
                title: 'Session Keys',
                subtitle: 'On-chain session key management',
              ),
              const SizedBox(height: 28),

              // Register New Key button
              SizedBox(
                width: double.infinity,
                child: GestureDetector(
                  onTap: _svc.isRegistering ? null : _registerNewKey,
                  child: Container(
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        colors: _svc.isRegistering
                            ? [kTextTertiary, kTextTertiary]
                            : [kNeonCyan, kNeonCyanDim],
                      ),
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        if (_svc.isRegistering)
                          const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: kBackground,
                            ),
                          )
                        else
                          const Icon(LucideIcons.plus, size: 18, color: kBackground),
                        const SizedBox(width: 8),
                        Text(
                          _svc.isRegistering ? 'Registering...' : 'Register New Key',
                          style: GoogleFonts.inter(
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                            color: kBackground,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
              const SizedBox(height: 24),

              // Keys list
              if (_isLoading)
                const Center(
                  child: Padding(
                    padding: EdgeInsets.only(top: 40),
                    child: CircularProgressIndicator(color: kNeonCyan),
                  ),
                )
              else if (_error != null)
                _buildErrorState()
              else if (_svc.sessionKeys.isEmpty)
                _buildEmptyState()
              else
                ..._svc.sessionKeys.map((key) => _buildKeyCard(key)),

              const SizedBox(height: 40),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildEmptyState() {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(32),
      decoration: glassDecoration(),
      child: Column(
        children: [
          Icon(LucideIcons.keyRound, size: 40, color: kTextTertiary),
          const SizedBox(height: 16),
          Text(
            'No session keys registered',
            style: GoogleFonts.inter(
              fontSize: 15,
              fontWeight: FontWeight.w600,
              color: kTextSecondary,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            'Register a new key to authorize payments on-chain',
            style: GoogleFonts.inter(fontSize: 12, color: kTextTertiary),
            textAlign: TextAlign.center,
          ),
        ],
      ),
    );
  }

  Widget _buildErrorState() {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: kSurfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: kDanger.withValues(alpha: 0.3)),
      ),
      child: Column(
        children: [
          Icon(LucideIcons.alertCircle, size: 32, color: kDanger),
          const SizedBox(height: 12),
          Text(
            'Failed to load session keys',
            style: GoogleFonts.inter(
              fontSize: 14,
              fontWeight: FontWeight.w600,
              color: kDanger,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            _error ?? 'Unknown error',
            style: GoogleFonts.inter(fontSize: 11, color: kTextSecondary),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 12),
          GestureDetector(
            onTap: _loadKeys,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              decoration: BoxDecoration(
                color: kDanger.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: kDanger.withValues(alpha: 0.3)),
              ),
              child: Text(
                'Retry',
                style: GoogleFonts.inter(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: kDanger,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildKeyCard(session.SessionKeyEntry key) {
    final isActive = key.status == 'active';
    final statusColor = isActive ? kSuccess : kDanger;

    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Container(
        padding: const EdgeInsets.all(16),
        decoration: glassDecoration(
          accentBorder: isActive ? kSuccess.withValues(alpha: 0.2) : null,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Container(
                  width: 36,
                  height: 36,
                  decoration: BoxDecoration(
                    color: statusColor.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: statusColor.withValues(alpha: 0.2)),
                  ),
                  child: Icon(LucideIcons.keyRound, size: 17, color: statusColor),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Text(
                            _shortenPubkey(key.ephemeralPubkey),
                            style: GoogleFonts.jetBrainsMono(
                              fontSize: 13,
                              fontWeight: FontWeight.w500,
                              color: kTextPrimary,
                            ),
                          ),
                          const SizedBox(width: 8),
                          _StatusBadge(color: statusColor, label: key.status),
                        ],
                      ),
                      const SizedBox(height: 4),
                      Text(
                        'Expires: ${_formatExpiry(key.expiresAt)}',
                        style: GoogleFonts.inter(fontSize: 11, color: kTextSecondary),
                      ),
                    ],
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Text(
                  'Limit: ',
                  style: GoogleFonts.inter(fontSize: 11, color: kTextSecondary),
                ),
                Text(
                  _formatLimit(key.spendingLimit),
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 12,
                    fontWeight: FontWeight.w500,
                    color: kTextPrimary,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                _ActionChip(
                  label: 'Revoke',
                  icon: LucideIcons.ban,
                  color: kDanger,
                  onTap: () => _revokeKey(key.ephemeralPubkey),
                ),
                const SizedBox(width: 8),
                _ActionChip(
                  label: 'Delete',
                  icon: LucideIcons.trash2,
                  color: kAmber,
                  onTap: () => _deleteKey(key.ephemeralPubkey),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Status Badge
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Action Chip
// ---------------------------------------------------------------------------
class _ActionChip extends StatelessWidget {
  final String label;
  final IconData icon;
  final Color color;
  final VoidCallback onTap;

  const _ActionChip({
    required this.label,
    required this.icon,
    required this.color,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.08),
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: color.withValues(alpha: 0.2)),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 13, color: color),
            const SizedBox(width: 6),
            Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: color,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
