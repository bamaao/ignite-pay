import 'dart:math';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';

// ---------------------------------------------------------------------------
// Vault Theme
// ---------------------------------------------------------------------------
const _kBackground = Color(0xFF0A0A14);
const _kSurface = Color(0xFF12121F);
const _kSurfaceElevated = Color(0xFF1A1A2E);
const _kBorder = Color(0xFF22223A);
const _kTextPrimary = Color(0xFFF0F0F8);
const _kTextSecondary = Color(0xFF7A7A96);
const _kTextTertiary = Color(0xFF4A4A64);
const _kPurple = Color(0xFF8B5CF6);
const _kPurpleDim = Color(0xFF6D28D9);
const _kBlue = Color(0xFF3B82F6);
const _kCyan = Color(0xFF06B6D4);
const _kSuccess = Color(0xFF00E676);
const _kDanger = Color(0xFFFF5252);
const _kAmber = Color(0xFFFFB300);
// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openVaultIdentity(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const VaultIdentityScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Vault Identity Screen
// ---------------------------------------------------------------------------
class VaultIdentityScreen extends StatefulWidget {
  const VaultIdentityScreen({super.key});

  @override
  State<VaultIdentityScreen> createState() => _VaultIdentityScreenState();
}

class _VaultIdentityScreenState extends State<VaultIdentityScreen> {
  bool _phraseRevealed = false;
  final _mediatorController = TextEditingController(
    text: 'wss://relay.ignite.did',
  );

  static const _secretPhrase = [
    'orbit', 'glacier', 'velvet', 'phoenix',
    'tundra', 'mirror', 'beacon', 'labyrinth',
    'cascade', 'ember', 'zenith', 'prism',
  ];

  @override
  void dispose() {
    _mediatorController.dispose();
    super.dispose();
  }

  void _triggerHaptic() {
    HapticFeedback.mediumImpact();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: _kBackground,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(height: 12),
              const _VaultHeader(),
              const SizedBox(height: 24),
              _IdentityHeroCard(onCopy: _triggerHaptic),
              const SizedBox(height: 28),
              _SectionLabel(text: 'VAULT'),
              const SizedBox(height: 8),
              _SecretPhraseTile(
                revealed: _phraseRevealed,
                onTap: () {
                  _triggerHaptic();
                  setState(() => _phraseRevealed = !_phraseRevealed);
                },
                phrase: _phraseRevealed ? _secretPhrase : null,
              ),
              const SizedBox(height: 8),
              _MediatorEndpointTile(controller: _mediatorController),
              const SizedBox(height: 8),
              _AuditLogSettingsTile(onTap: _triggerHaptic),
              const SizedBox(height: 8),
              _DangerZoneTile(onTap: _triggerHaptic),
              const SizedBox(height: 40),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------
class _VaultHeader extends StatelessWidget {
  const _VaultHeader();

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        GestureDetector(
          onTap: () => Navigator.of(context).pop(),
          child: Container(
            width: 36,
            height: 36,
            decoration: BoxDecoration(
              color: _kSurface,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: _kBorder),
            ),
            child: const Icon(LucideIcons.arrowLeft, size: 18, color: _kTextSecondary),
          ),
        ),
        const SizedBox(width: 14),
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Vault & Identity',
              style: GoogleFonts.inter(
                fontSize: 20,
                fontWeight: FontWeight.w700,
                color: _kTextPrimary,
                letterSpacing: -0.3,
              ),
            ),
            Text(
              'Key management & credentials',
              style: GoogleFonts.inter(
                fontSize: 12,
                color: _kTextSecondary,
              ),
            ),
          ],
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Hero Identity Card (Mesh Gradient)
// ---------------------------------------------------------------------------
class _IdentityHeroCard extends StatefulWidget {
  final VoidCallback onCopy;

  const _IdentityHeroCard({required this.onCopy});

  @override
  State<_IdentityHeroCard> createState() => _IdentityHeroCardState();
}

class _IdentityHeroCardState extends State<_IdentityHeroCard>
    with SingleTickerProviderStateMixin {
  late final AnimationController _meshCtrl;
  bool _copied = false;

  String get _did => DidcommService().did.isNotEmpty
      ? DidcommService().did
      : 'did:ignite:zInitializing...';

  @override
  void initState() {
    super.initState();
    _meshCtrl = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 8000),
    )..repeat();
  }

  @override
  void dispose() {
    _meshCtrl.dispose();
    super.dispose();
  }

  void _copyDid() {
    Clipboard.setData(ClipboardData(text: _did));
    widget.onCopy();
    setState(() => _copied = true);
    Future.delayed(const Duration(seconds: 2), () {
      if (mounted) setState(() => _copied = false);
    });
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _meshCtrl,
      builder: (context, child) {
        final t = _meshCtrl.value * 2 * pi;
        return Container(
          width: double.infinity,
          padding: const EdgeInsets.all(24),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(20),
            gradient: LinearGradient(
              begin: Alignment(sin(t) * 0.5, cos(t) * 0.5),
              end: Alignment(-sin(t + 1) * 0.6, cos(t + 2) * 0.6),
              colors: [
                _kPurpleDim,
                _kPurple.withValues(alpha: 0.7),
                _kBlue.withValues(alpha: 0.6),
                _kCyan.withValues(alpha: 0.3),
                _kPurpleDim.withValues(alpha: 0.8),
              ],
              stops: const [0.0, 0.25, 0.5, 0.75, 1.0],
            ),
            border: Border.all(color: _kPurple.withValues(alpha: 0.3)),
            boxShadow: [
              BoxShadow(
                color: _kPurple.withValues(alpha: 0.15),
                blurRadius: 30,
                spreadRadius: 4,
              ),
            ],
          ),
          child: child,
        );
      },
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                decoration: BoxDecoration(
                  color: Colors.black.withValues(alpha: 0.3),
                  borderRadius: BorderRadius.circular(6),
                  border: Border.all(color: Colors.white.withValues(alpha: 0.1)),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(LucideIcons.fingerprint, size: 14, color: Colors.white70),
                    const SizedBox(width: 6),
                    Text(
                      'DECENTRALIZED IDENTITY',
                      style: GoogleFonts.inter(
                        fontSize: 9,
                        fontWeight: FontWeight.w700,
                        color: Colors.white.withValues(alpha: 0.8),
                        letterSpacing: 1.0,
                      ),
                    ),
                  ],
                ),
              ),
              const Spacer(),
              // Hardware Protected badge
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                decoration: BoxDecoration(
                  color: _kSuccess.withValues(alpha: 0.15),
                  borderRadius: BorderRadius.circular(6),
                  border: Border.all(color: _kSuccess.withValues(alpha: 0.35)),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(LucideIcons.shieldCheck, size: 12, color: _kSuccess.withValues(alpha: 0.9)),
                    const SizedBox(width: 5),
                    Text(
                      'HW Protected',
                      style: GoogleFonts.inter(
                        fontSize: 9,
                        fontWeight: FontWeight.w700,
                        color: _kSuccess.withValues(alpha: 0.9),
                        letterSpacing: 0.5,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 20),
          // DID display
          Row(
            children: [
              Expanded(
                child: Text(
                  _did,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 15,
                    fontWeight: FontWeight.w500,
                    color: _kTextPrimary,
                    height: 1.4,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              GestureDetector(
                onTap: _copyDid,
                child: Container(
                  width: 34,
                  height: 34,
                  decoration: BoxDecoration(
                    color: Colors.white.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(
                      color: _copied
                          ? _kSuccess.withValues(alpha: 0.5)
                          : Colors.white.withValues(alpha: 0.15),
                    ),
                  ),
                  child: Icon(
                    _copied ? LucideIcons.check : LucideIcons.copy,
                    size: 15,
                    color: _copied ? _kSuccess : Colors.white70,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          // Key metadata row
          Row(
            children: [
              _KeyMetaChip(label: 'Ed25519', icon: LucideIcons.keyRound),
              const SizedBox(width: 8),
              _KeyMetaChip(label: 'Mainnet', icon: LucideIcons.globe),
              const SizedBox(width: 8),
              _KeyMetaChip(label: 'Active', icon: LucideIcons.radio),
            ],
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Key Meta Chip
// ---------------------------------------------------------------------------
class _KeyMetaChip extends StatelessWidget {
  final String label;
  final IconData icon;

  const _KeyMetaChip({required this.label, required this.icon});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: Colors.black.withValues(alpha: 0.25),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 10, color: Colors.white54),
          const SizedBox(width: 4),
          Text(
            label,
            style: GoogleFonts.inter(
              fontSize: 9,
              fontWeight: FontWeight.w600,
              color: Colors.white60,
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Section Label
// ---------------------------------------------------------------------------
class _SectionLabel extends StatelessWidget {
  final String text;

  const _SectionLabel({required this.text});

  @override
  Widget build(BuildContext context) {
    return Text(
      text,
      style: GoogleFonts.inter(
        fontSize: 10,
        fontWeight: FontWeight.w700,
        color: _kTextTertiary,
        letterSpacing: 1.5,
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Settings Tile Base (Swiss-grid style)
// ---------------------------------------------------------------------------
class _SettingsTile extends StatelessWidget {
  final IconData icon;
  final Color iconColor;
  final String title;
  final String? subtitle;
  final Widget trailing;
  final VoidCallback? onTap;
  final Color? accentBorder;

  const _SettingsTile({
    required this.icon,
    required this.iconColor,
    required this.title,
    this.subtitle,
    required this.trailing,
    this.onTap,
    this.accentBorder,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(
          color: _kSurface,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: accentBorder ?? _kBorder,
          ),
        ),
        child: Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                color: iconColor.withValues(alpha: 0.08),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                  color: iconColor.withValues(alpha: 0.15),
                ),
              ),
              child: Icon(icon, size: 17, color: iconColor),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: GoogleFonts.inter(
                      fontSize: 14,
                      fontWeight: FontWeight.w600,
                      color: _kTextPrimary,
                    ),
                  ),
                  if (subtitle != null) ...[
                    const SizedBox(height: 2),
                    Text(
                      subtitle!,
                      style: GoogleFonts.inter(
                        fontSize: 11,
                        color: _kTextSecondary,
                      ),
                    ),
                  ],
                ],
              ),
            ),
            trailing,
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Secret Phrase Tile (Masked / Revealed)
// ---------------------------------------------------------------------------
class _SecretPhraseTile extends StatelessWidget {
  final bool revealed;
  final VoidCallback onTap;
  final List<String>? phrase;

  const _SecretPhraseTile({
    required this.revealed,
    required this.onTap,
    this.phrase,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        _SettingsTile(
          icon: LucideIcons.lock,
          iconColor: _kAmber,
          title: 'Back up Secret Phrase',
          subtitle: revealed ? 'Tap to hide' : '12-word recovery phrase',
          onTap: onTap,
          accentBorder: revealed ? _kAmber.withValues(alpha: 0.25) : null,
          trailing: Icon(
            revealed ? LucideIcons.eyeOff : LucideIcons.eye,
            size: 18,
            color: _kAmber.withValues(alpha: 0.7),
          ),
        ),
        if (revealed && phrase != null) ...[
          const SizedBox(height: 8),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              color: _kSurface,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: _kAmber.withValues(alpha: 0.2)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(LucideIcons.alertTriangle, size: 13, color: _kAmber.withValues(alpha: 0.8)),
                    const SizedBox(width: 6),
                    Text(
                      'NEVER SHARE THESE WORDS',
                      style: GoogleFonts.inter(
                        fontSize: 9,
                        fontWeight: FontWeight.w700,
                        color: _kAmber,
                        letterSpacing: 1.0,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (int i = 0; i < phrase!.length; i++)
                      _WordChip(number: i + 1, word: phrase![i]),
                  ],
                ),
              ],
            ),
          ),
        ],
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Word Chip
// ---------------------------------------------------------------------------
class _WordChip extends StatelessWidget {
  final int number;
  final String word;

  const _WordChip({required this.number, required this.word});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 5),
      decoration: BoxDecoration(
        color: _kSurfaceElevated,
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: _kBorder),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          SizedBox(
            width: 18,
            child: Text(
              number.toString().padLeft(2, '0'),
              style: GoogleFonts.jetBrainsMono(
                fontSize: 9,
                fontWeight: FontWeight.w600,
                color: _kTextTertiary,
              ),
            ),
          ),
          const SizedBox(width: 4),
          Text(
            word,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 12,
              fontWeight: FontWeight.w500,
              color: _kTextPrimary,
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Mediator Endpoint Tile
// ---------------------------------------------------------------------------
class _MediatorEndpointTile extends StatefulWidget {
  final TextEditingController controller;

  const _MediatorEndpointTile({required this.controller});

  @override
  State<_MediatorEndpointTile> createState() => _MediatorEndpointTileState();
}

class _MediatorEndpointTileState extends State<_MediatorEndpointTile> {
  bool _editing = false;

  @override
  Widget build(BuildContext context) {
    return _SettingsTile(
      icon: LucideIcons.radio,
      iconColor: _kCyan,
      title: 'Mediator Endpoint',
      subtitle: 'WebSocket relay for DIDComm',
      onTap: () {
        HapticFeedback.lightImpact();
        setState(() => _editing = !_editing);
      },
      accentBorder: _editing ? _kCyan.withValues(alpha: 0.25) : null,
      trailing: Icon(
        _editing ? LucideIcons.check : LucideIcons.settings2,
        size: 18,
        color: _kCyan.withValues(alpha: 0.7),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Audit Log Tile
// ---------------------------------------------------------------------------
class _AuditLogSettingsTile extends StatelessWidget {
  final VoidCallback onTap;

  const _AuditLogSettingsTile({required this.onTap});

  @override
  Widget build(BuildContext context) {
    return _SettingsTile(
      icon: LucideIcons.fileSearch,
      iconColor: _kPurple,
      title: 'Signature Audit Logs',
      subtitle: '3 events this week',
      onTap: () {
        onTap();
        openAuditLogs(context);
      },
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            decoration: BoxDecoration(
              color: _kPurple.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(4),
            ),
            child: Text(
              '3',
              style: GoogleFonts.jetBrainsMono(
                fontSize: 10,
                fontWeight: FontWeight.w600,
                color: _kPurple,
              ),
            ),
          ),
          const SizedBox(width: 8),
          const Icon(LucideIcons.chevronRight, size: 18, color: _kTextTertiary),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Danger Zone Tile
// ---------------------------------------------------------------------------
class _DangerZoneTile extends StatelessWidget {
  final VoidCallback onTap;

  const _DangerZoneTile({required this.onTap});

  @override
  Widget build(BuildContext context) {
    return _SettingsTile(
      icon: LucideIcons.alertTriangle,
      iconColor: _kDanger,
      title: 'Erase Key Material',
      subtitle: 'Permanently delete local keys',
      onTap: onTap,
      accentBorder: _kDanger.withValues(alpha: 0.15),
      trailing: Icon(
        LucideIcons.trash2,
        size: 18,
        color: _kDanger.withValues(alpha: 0.5),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Audit Logs Nested Page
// ---------------------------------------------------------------------------
void openAuditLogs(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 300),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const _AuditLogsPage(),
      ),
    ),
  );
}

class _AuditLogsPage extends StatelessWidget {
  const _AuditLogsPage();

  static const _logs = [
    _AuditEntry(
      action: 'sign_payment',
      merchant: 'ShopX Marketplace',
      amount: '0.12 SOL',
      time: 'Apr 9, 14:32',
      status: 'confirmed',
    ),
    _AuditEntry(
      action: 'sign_payment',
      merchant: 'DeFi Staking',
      amount: '0.30 SOL',
      time: 'Apr 9, 12:15',
      status: 'pending',
    ),
    _AuditEntry(
      action: 'key_derive',
      merchant: 'System',
      amount: '',
      time: 'Apr 8, 09:00',
      status: 'confirmed',
    ),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: _kBackground,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 12, 20, 16),
              child: Row(
                children: [
                  GestureDetector(
                    onTap: () => Navigator.of(context).pop(),
                    child: Container(
                      width: 36,
                      height: 36,
                      decoration: BoxDecoration(
                        color: _kSurface,
                        borderRadius: BorderRadius.circular(8),
                        border: Border.all(color: _kBorder),
                      ),
                      child: const Icon(LucideIcons.arrowLeft, size: 18, color: _kTextSecondary),
                    ),
                  ),
                  const SizedBox(width: 14),
                  Text(
                    'Signature Audit Logs',
                    style: GoogleFonts.inter(
                      fontSize: 18,
                      fontWeight: FontWeight.w700,
                      color: _kTextPrimary,
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20),
              child: Text(
                'CRYPTOGRAPHIC PROOF OF AUTHORIZATION',
                style: GoogleFonts.inter(
                  fontSize: 9,
                  fontWeight: FontWeight.w700,
                  color: _kTextTertiary,
                  letterSpacing: 1.2,
                ),
              ),
            ),
            const SizedBox(height: 12),
            Expanded(
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(horizontal: 20),
                itemCount: _logs.length,
                separatorBuilder: (_, _) => const SizedBox(height: 6),
                itemBuilder: (context, index) => _AuditLogEntryTile(entry: _logs[index]),
              ),
            ),
            const SizedBox(height: 20),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Audit Entry Model
// ---------------------------------------------------------------------------
class _AuditEntry {
  final String action;
  final String merchant;
  final String amount;
  final String time;
  final String status;

  const _AuditEntry({
    required this.action,
    required this.merchant,
    required this.amount,
    required this.time,
    required this.status,
  });
}

// ---------------------------------------------------------------------------
// Audit Log Entry Tile
// ---------------------------------------------------------------------------
class _AuditLogEntryTile extends StatelessWidget {
  final _AuditEntry entry;

  const _AuditLogEntryTile({required this.entry});

  IconData get _icon => switch (entry.action) {
        'sign_payment' => LucideIcons.penTool,
        'key_derive' => LucideIcons.keyRound,
        _ => LucideIcons.file,
      };

  Color get _statusColor => entry.status == 'confirmed' ? _kSuccess : _kAmber;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: _kSurface,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: _kBorder),
      ),
      child: Row(
        children: [
          Container(
            width: 32,
            height: 32,
            decoration: BoxDecoration(
              color: _statusColor.withValues(alpha: 0.08),
              borderRadius: BorderRadius.circular(8),
            ),
            child: Icon(_icon, size: 15, color: _statusColor),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  entry.merchant,
                  style: GoogleFonts.inter(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: _kTextPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  '${entry.action}  ${entry.amount}',
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 10,
                    color: _kTextSecondary,
                  ),
                ),
              ],
            ),
          ),
          Column(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Text(
                entry.time,
                style: GoogleFonts.inter(
                  fontSize: 10,
                  color: _kTextTertiary,
                ),
              ),
              const SizedBox(height: 3),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                decoration: BoxDecoration(
                  color: _statusColor.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(4),
                ),
                child: Text(
                  entry.status.toUpperCase(),
                  style: GoogleFonts.inter(
                    fontSize: 8,
                    fontWeight: FontWeight.w700,
                    color: _statusColor,
                    letterSpacing: 0.5,
                  ),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
