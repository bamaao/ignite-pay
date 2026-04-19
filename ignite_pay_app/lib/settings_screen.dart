import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/connection_screen.dart';
import 'package:ignite_pay_app/vault_screen.dart';
import 'package:ignite_pay_app/policy_screen.dart';
import 'package:ignite_pay_app/session_keys_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openSettings(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const SettingsScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Settings Screen
// ---------------------------------------------------------------------------
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  final _rpcController = TextEditingController();
  final _treeAddrController = TextEditingController();
  final _treeAuthController = TextEditingController();
  final _dasController = TextEditingController();
  String _payMode = 'self_funded';
  String _network = 'devnet';

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  Future<void> _loadSettings() async {
    final prefs = await SharedPreferences.getInstance();
    setState(() {
      _rpcController.text = prefs.getString('solana_rpc_url') ?? 'https://api.devnet.solana.com';
      _treeAddrController.text = prefs.getString('tree_address') ?? '';
      _treeAuthController.text = prefs.getString('tree_authority') ?? '';
      _dasController.text = prefs.getString('das_endpoint') ?? '';
      _payMode = prefs.getString('pay_mode') ?? 'self_funded';
      _network = prefs.getString('network') ?? 'devnet';
    });
  }

  Future<void> _saveSetting(String key, String value) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(key, value);
  }

  void _showClearConfirm() {
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: kSurfaceDark,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
        title: Text('Clear Cache?',
            style: GoogleFonts.inter(
                fontSize: 16, fontWeight: FontWeight.w700, color: kTextPrimary)),
        content: Text(
          'This will clear cached messages and temporary data. Your DID keys will not be affected.',
          style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: Text('Cancel',
                style: GoogleFonts.inter(color: kTextSecondary)),
          ),
          TextButton(
            onPressed: () {
              Navigator.of(ctx).pop();
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                  backgroundColor: kSuccess,
                  behavior: SnackBarBehavior.floating,
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(10)),
                  margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                  content: Text('Cache cleared',
                      style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
                ),
              );
            },
            child: Text('Clear',
                style: GoogleFonts.inter(color: kDanger)),
          ),
        ],
      ),
    );
  }

  @override
  void dispose() {
    _rpcController.dispose();
    _treeAddrController.dispose();
    _treeAuthController.dispose();
    _dasController.dispose();
    super.dispose();
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
              const PageHeader(title: 'Settings', subtitle: 'Configure Ignite Pay'),
              const SizedBox(height: 28),

              // Quick links
              const SectionLabel(text: 'QUICK ACCESS'),
              const SizedBox(height: 8),
              SettingsTile(
                icon: LucideIcons.lock,
                iconColor: kPurple,
                title: 'Vault & Identity',
                subtitle: 'Keys, mnemonic, audit logs',
                trailing: const Icon(LucideIcons.chevronRight,
                    size: 18, color: kTextTertiary),
                onTap: () => openVaultIdentity(context),
              ),
              const SizedBox(height: 6),
              SettingsTile(
                icon: LucideIcons.shield,
                iconColor: kNeonCyan,
                title: 'Policy Architect',
                subtitle: 'Spending rules & limits',
                trailing: const Icon(LucideIcons.chevronRight,
                    size: 18, color: kTextTertiary),
                onTap: () => openPolicyArchitect(context),
              ),
              const SizedBox(height: 6),
              SettingsTile(
                icon: LucideIcons.radio,
                iconColor: kCyan,
                title: 'Connections',
                subtitle: 'Mediator & MCP management',
                trailing: const Icon(LucideIcons.chevronRight,
                    size: 18, color: kTextTertiary),
                onTap: () => openConnectionManagement(context),
              ),
              const SizedBox(height: 6),
              SettingsTile(
                icon: LucideIcons.keyRound,
                iconColor: kPurple,
                title: 'Session Keys',
                subtitle: 'On-chain session key management',
                trailing: const Icon(LucideIcons.chevronRight,
                    size: 18, color: kTextTertiary),
                onTap: () => openSessionKeys(context),
              ),
              const SizedBox(height: 24),

              // Solana Network
              const SectionLabel(text: 'SOLANA NETWORK'),
              const SizedBox(height: 8),
              _NetworkSelector(
                current: _network,
                onChanged: (v) {
                  setState(() => _network = v);
                  _saveSetting('network', v);
                  // Update RPC URL based on network
                  final rpc = v == 'mainnet-beta'
                      ? 'https://api.mainnet-beta.solana.com'
                      : 'https://api.devnet.solana.com';
                  _rpcController.text = rpc;
                  _saveSetting('solana_rpc_url', rpc);
                },
              ),
              const SizedBox(height: 8),
              _ConfigField(
                label: 'RPC URL',
                controller: _rpcController,
                onChanged: (v) => _saveSetting('solana_rpc_url', v),
              ),
              const SizedBox(height: 8),
              _ConfigField(
                label: 'DAS Endpoint',
                controller: _dasController,
                placeholder: 'https://rpc.helius.dev',
                onChanged: (v) => _saveSetting('das_endpoint', v),
              ),
              const SizedBox(height: 24),

              // SPL Compression
              const SectionLabel(text: 'SPL ACCOUNT COMPRESSION'),
              const SizedBox(height: 8),
              _ConfigField(
                label: 'Tree Address',
                controller: _treeAddrController,
                placeholder: 'Merkle tree account',
                onChanged: (v) => _saveSetting('tree_address', v),
              ),
              const SizedBox(height: 8),
              _ConfigField(
                label: 'Tree Authority',
                controller: _treeAuthController,
                placeholder: 'Tree authority PDA',
                onChanged: (v) => _saveSetting('tree_authority', v),
              ),
              const SizedBox(height: 24),

              // Program IDs
              const SectionLabel(text: 'PROGRAM IDS'),
              const SizedBox(height: 8),
              _ReadOnlyField(
                label: 'State Channel',
                value: 'DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe',
              ),
              const SizedBox(height: 6),
              _ReadOnlyField(
                label: 'DID (ZK Compression)',
                value: 'ignDID... (see deploy docs)',
              ),
              const SizedBox(height: 6),
              _ReadOnlyField(
                label: 'Session Key',
                value: '6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei',
              ),
              const SizedBox(height: 24),

              // Payment Mode
              const SectionLabel(text: 'PAYMENT MODE'),
              const SizedBox(height: 8),
              _PayModeSelector(
                current: _payMode,
                onChanged: (v) {
                  setState(() => _payMode = v);
                  _saveSetting('pay_mode', v);
                },
              ),
              const SizedBox(height: 24),

              // Storage
              const SectionLabel(text: 'STORAGE'),
              const SizedBox(height: 8),
              SettingsTile(
                icon: LucideIcons.trash,
                iconColor: kAmber,
                title: 'Clear Cache',
                subtitle: 'Remove cached messages and temp data',
                trailing: Icon(LucideIcons.chevronRight,
                    size: 18, color: kTextTertiary),
                onTap: _showClearConfirm,
              ),
              const SizedBox(height: 24),

              // About
              const SectionLabel(text: 'ABOUT'),
              const SizedBox(height: 8),
              SettingsTile(
                icon: LucideIcons.info,
                iconColor: kTextSecondary,
                title: 'Ignite Pay Sentinel',
                subtitle: 'Version 1.0.0 (build 1)',
                trailing: Icon(LucideIcons.externalLink,
                    size: 16, color: kTextTertiary),
              ),
              const SizedBox(height: 40),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Network Selector
// ---------------------------------------------------------------------------
class _NetworkSelector extends StatelessWidget {
  final String current;
  final ValueChanged<String> onChanged;

  const _NetworkSelector({required this.current, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        _networkChip('devnet', current == 'devnet'),
        const SizedBox(width: 8),
        _networkChip('mainnet-beta', current == 'mainnet-beta'),
      ],
    );
  }

  Widget _networkChip(String value, bool selected) {
    final color = selected
        ? (value == 'mainnet-beta' ? kSuccess : kNeonCyan)
        : kTextTertiary;
    return Expanded(
      child: GestureDetector(
        onTap: () => onChanged(value),
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 10),
          decoration: BoxDecoration(
            color: selected ? color.withValues(alpha: 0.1) : kSurfaceDark,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(
              color: selected ? color.withValues(alpha: 0.3) : kBorder,
            ),
          ),
          child: Center(
            child: Text(
              value,
              style: GoogleFonts.inter(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: color,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Payment Mode Selector
// ---------------------------------------------------------------------------
class _PayModeSelector extends StatelessWidget {
  final String current;
  final ValueChanged<String> onChanged;

  const _PayModeSelector({required this.current, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: _modeChip('Self-Funded', 'self_funded', current == 'self_funded'),
        ),
        const SizedBox(width: 8),
        Expanded(
          child: _modeChip('Sponsored', 'sponsored', current == 'sponsored'),
        ),
      ],
    );
  }

  Widget _modeChip(String label, String value, bool selected) {
    final color = selected ? kNeonCyan : kTextTertiary;
    return GestureDetector(
      onTap: () => onChanged(value),
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 10),
        decoration: BoxDecoration(
          color: selected ? color.withValues(alpha: 0.1) : kSurfaceDark,
          borderRadius: BorderRadius.circular(10),
          border: Border.all(
            color: selected ? color.withValues(alpha: 0.3) : kBorder,
          ),
        ),
        child: Center(
          child: Text(
            label,
            style: GoogleFonts.inter(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: color,
            ),
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Config Field (editable)
// ---------------------------------------------------------------------------
class _ConfigField extends StatelessWidget {
  final String label;
  final TextEditingController controller;
  final String? placeholder;
  final ValueChanged<String>? onChanged;

  const _ConfigField({
    required this.label,
    required this.controller,
    this.placeholder,
    this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label.toUpperCase(), style: sectionLabel()),
        const SizedBox(height: 4),
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          decoration: BoxDecoration(
            color: kSurfaceMid,
            borderRadius: BorderRadius.circular(8),
            border: Border.all(color: kBorder),
          ),
          child: TextField(
            controller: controller,
            onChanged: onChanged,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 12,
              color: kTextPrimary,
            ),
            decoration: InputDecoration(
              border: InputBorder.none,
              hintText: placeholder ?? '',
              hintStyle: GoogleFonts.jetBrainsMono(
                fontSize: 12,
                color: kTextTertiary,
              ),
              isDense: true,
              contentPadding: const EdgeInsets.symmetric(vertical: 10),
            ),
          ),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Read Only Field
// ---------------------------------------------------------------------------
class _ReadOnlyField extends StatelessWidget {
  final String label;
  final String value;

  const _ReadOnlyField({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: GoogleFonts.inter(
            fontSize: 11, fontWeight: FontWeight.w600, color: kTextSecondary,
          )),
          const SizedBox(height: 4),
          SelectableText(
            value,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 10, color: kTextTertiary,
            ),
          ),
        ],
      ),
    );
  }
}
