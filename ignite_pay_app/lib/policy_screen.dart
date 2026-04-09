import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';

// ---------------------------------------------------------------------------
// Swiss Design Theme Constants
// ---------------------------------------------------------------------------
const _kBackground = Color(0xFF0F0F1A);
const _kSurface = Color(0xFF161623);
const _kBorder = Color(0xFF2A2A3E);
const _kTextPrimary = Color(0xFFE8E8F0);
const _kTextSecondary = Color(0xFF7A7A92);
const _kTextTertiary = Color(0xFF55556A);
const _kNeonCyan = Color(0xFF00F5FF);
const _kSuccess = Color(0xFF00E676);
const _kPending = Color(0xFFFFB300);
const _kIntercepted = Color(0xFFFF5252);
const _kActiveGreen = Color(0xFF00E676);
const _kInactiveGray = Color(0xFF3A3A50);

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
void openPolicyArchitect(BuildContext context) {
  Navigator.of(context).push(
    PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 350),
      pageBuilder: (_, animation, _) => SlideTransition(
        position: Tween<Offset>(
          begin: const Offset(1, 0),
          end: Offset.zero,
        ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
        child: const PolicyArchitectScreen(),
      ),
    ),
  );
}

// ---------------------------------------------------------------------------
// Policy Data Models
// ---------------------------------------------------------------------------
class _MerchantPolicy {
  final String name;
  final String did;
  final String domain;
  final IconData icon;
  final bool isVerified;
  bool autoPay;
  double singleLimit;
  double weeklyCap;
  double weeklySpent;
  DateTime expiry;
  bool isExpanded = false;

  _MerchantPolicy({
    required this.name,
    required this.did,
    required this.domain,
    required this.icon,
    this.isVerified = false,
    this.autoPay = false,
    this.singleLimit = 0.5,
    this.weeklyCap = 2.0,
    this.weeklySpent = 0.0,
    required this.expiry,
  });
}

// ---------------------------------------------------------------------------
// Policy Architect Screen
// ---------------------------------------------------------------------------
class PolicyArchitectScreen extends StatefulWidget {
  const PolicyArchitectScreen({super.key});

  @override
  State<PolicyArchitectScreen> createState() => _PolicyArchitectScreenState();
}

class _PolicyArchitectScreenState extends State<PolicyArchitectScreen> {
  late final List<_MerchantPolicy> _policies;

  @override
  void initState() {
    super.initState();
    _policies = [
      _MerchantPolicy(
        name: 'ShopX Marketplace',
        did: 'did:solana:7kPx...mN3q',
        domain: 'shopx.io',
        icon: LucideIcons.store,
        isVerified: true,
        autoPay: true,
        singleLimit: 0.5,
        weeklyCap: 2.0,
        weeklySpent: 0.72,
        expiry: DateTime(2025, 8, 15),
      ),
      _MerchantPolicy(
        name: 'DeFi Staking',
        did: 'did:solana:3vRt...kL9w',
        domain: 'defistake.xyz',
        icon: LucideIcons.layers,
        isVerified: false,
        autoPay: false,
        singleLimit: 1.0,
        weeklyCap: 5.0,
        weeklySpent: 3.2,
        expiry: DateTime(2025, 6, 30),
      ),
      _MerchantPolicy(
        name: 'NFT Mint',
        did: 'did:solana:9wYe...pQ2a',
        domain: 'nftmint.pro',
        icon: LucideIcons.image,
        isVerified: true,
        autoPay: false,
        singleLimit: 0.25,
        weeklyCap: 1.0,
        weeklySpent: 0.0,
        expiry: DateTime(2025, 12, 1),
      ),
      _MerchantPolicy(
        name: 'RPC Provider',
        did: 'did:solana:5tUu...xZ7b',
        domain: 'solrpc.dev',
        icon: LucideIcons.server,
        isVerified: true,
        autoPay: true,
        singleLimit: 0.05,
        weeklyCap: 0.3,
        weeklySpent: 0.12,
        expiry: DateTime(2026, 1, 1),
      ),
    ];
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: _kBackground,
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const _PolicyHeader(),
            // Grid stats
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20),
              child: _StatsGrid(policies: _policies),
            ),
            const SizedBox(height: 20),
            // Merchant list
            Expanded(
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(horizontal: 20),
                itemCount: _policies.length,
                separatorBuilder: (_, _) => const SizedBox(height: 8),
                itemBuilder: (context, index) {
                  return _MerchantPolicyCard(
                    policy: _policies[index],
                    onToggleAutoPay: (val) {
                      setState(() => _policies[index].autoPay = val);
                    },
                    onToggleExpand: () {
                      setState(() {
                        _policies[index].isExpanded =
                            !_policies[index].isExpanded;
                      });
                    },
                    onLimitChanged: (val) {
                      setState(() => _policies[index].singleLimit = val);
                    },
                    onWeeklyCapChanged: (val) {
                      setState(() => _policies[index].weeklyCap = val);
                    },
                    onExpiryChanged: (val) {
                      setState(() => _policies[index].expiry = val);
                    },
                  );
                },
              ),
            ),
            const SizedBox(height: 16),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------
class _PolicyHeader extends StatelessWidget {
  const _PolicyHeader();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 20),
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
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Policy Architect',
                style: GoogleFonts.inter(
                  fontSize: 20,
                  fontWeight: FontWeight.w700,
                  color: _kTextPrimary,
                  letterSpacing: -0.3,
                ),
              ),
              Text(
                'Spending rules & whitelists',
                style: GoogleFonts.inter(
                  fontSize: 12,
                  color: _kTextSecondary,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Stats Grid (Swiss-style 2x2)
// ---------------------------------------------------------------------------
class _StatsGrid extends StatelessWidget {
  final List<_MerchantPolicy> policies;

  const _StatsGrid({required this.policies});

  @override
  Widget build(BuildContext context) {
    final active = policies.where((p) => p.autoPay).length;
    final totalCap = policies.fold<double>(0, (sum, p) => sum + p.weeklyCap);
    final totalSpent = policies.fold<double>(0, (sum, p) => sum + p.weeklySpent);

    return Container(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: _kBorder),
      ),
      child: Column(
        children: [
          Row(
            children: [
              _StatCell(
                label: 'MERCHANTS',
                value: policies.length.toString(),
                icon: LucideIcons.store,
                borderRight: true,
              ),
              _StatCell(
                label: 'AUTO-PAY',
                value: '$active',
                icon: LucideIcons.zap,
                valueColor: active > 0 ? _kActiveGreen : null,
              ),
            ],
          ),
          Container(height: 1, color: _kBorder),
          Row(
            children: [
              _StatCell(
                label: 'WEEKLY CAP',
                value: '${totalCap.toStringAsFixed(1)} SOL',
                icon: LucideIcons.gauge,
                borderRight: true,
              ),
              _StatCell(
                label: 'SPENT',
                value: '${totalSpent.toStringAsFixed(2)} SOL',
                icon: LucideIcons.trendingUp,
                valueColor: totalSpent > totalCap * 0.8 ? _kPending : null,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _StatCell extends StatelessWidget {
  final String label;
  final String value;
  final IconData icon;
  final Color? valueColor;
  final bool borderRight;

  const _StatCell({
    required this.label,
    required this.value,
    required this.icon,
    this.valueColor,
    this.borderRight = false,
  });

  @override
  Widget build(BuildContext context) {
    return Expanded(
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
        decoration: borderRight
            ? const BoxDecoration(
                border: Border(right: BorderSide(color: _kBorder)),
              )
            : null,
        child: Row(
          children: [
            Icon(icon, size: 14, color: _kTextTertiary),
            const SizedBox(width: 8),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: GoogleFonts.inter(
                    fontSize: 9,
                    fontWeight: FontWeight.w600,
                    color: _kTextTertiary,
                    letterSpacing: 1.0,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  value,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 14,
                    fontWeight: FontWeight.w600,
                    color: valueColor ?? _kTextPrimary,
                  ),
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
// Merchant Policy Card
// ---------------------------------------------------------------------------
class _MerchantPolicyCard extends StatelessWidget {
  final _MerchantPolicy policy;
  final ValueChanged<bool> onToggleAutoPay;
  final VoidCallback onToggleExpand;
  final ValueChanged<double> onLimitChanged;
  final ValueChanged<double> onWeeklyCapChanged;
  final ValueChanged<DateTime> onExpiryChanged;

  const _MerchantPolicyCard({
    required this.policy,
    required this.onToggleAutoPay,
    required this.onToggleExpand,
    required this.onLimitChanged,
    required this.onWeeklyCapChanged,
    required this.onExpiryChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        color: _kSurface,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: policy.isExpanded ? _kNeonCyan.withValues(alpha: 0.25) : _kBorder,
        ),
      ),
      child: Column(
        children: [
          // Main row
          GestureDetector(
            onTap: onToggleExpand,
            behavior: HitTestBehavior.opaque,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(14, 14, 10, 14),
              child: Row(
                children: [
                  // Merchant icon
                  Container(
                    width: 38,
                    height: 38,
                    decoration: BoxDecoration(
                      color: _kNeonCyan.withValues(alpha: 0.08),
                      borderRadius: BorderRadius.circular(8),
                      border: Border.all(color: _kBorder),
                    ),
                    child: Icon(policy.icon, size: 18, color: _kNeonCyan),
                  ),
                  const SizedBox(width: 12),
                  // Name + domain
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Text(
                              policy.name,
                              style: GoogleFonts.inter(
                                fontSize: 14,
                                fontWeight: FontWeight.w600,
                                color: _kTextPrimary,
                              ),
                            ),
                            if (policy.isVerified) ...[
                              const SizedBox(width: 6),
                              Icon(
                                LucideIcons.badgeCheck,
                                size: 13,
                                color: _kSuccess.withValues(alpha: 0.8),
                              ),
                            ],
                          ],
                        ),
                        const SizedBox(height: 2),
                        Text(
                          policy.domain,
                          style: GoogleFonts.inter(
                            fontSize: 11,
                            color: _kTextTertiary,
                          ),
                        ),
                      ],
                    ),
                  ),
                  // Auto-pay toggle
                  Column(
                    children: [
                      _SwissToggle(
                        value: policy.autoPay,
                        onChanged: onToggleAutoPay,
                      ),
                      const SizedBox(height: 2),
                      Text(
                        policy.autoPay ? 'AUTO' : 'MANUAL',
                        style: GoogleFonts.inter(
                          fontSize: 8,
                          fontWeight: FontWeight.w600,
                          color: policy.autoPay
                              ? _kActiveGreen.withValues(alpha: 0.7)
                              : _kTextTertiary,
                          letterSpacing: 0.8,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(width: 8),
                  // Expand chevron
                  AnimatedRotation(
                    turns: policy.isExpanded ? 0.25 : 0,
                    duration: const Duration(milliseconds: 200),
                    child: Icon(
                      LucideIcons.chevronRight,
                      size: 18,
                      color: _kTextTertiary,
                    ),
                  ),
                ],
              ),
            ),
          ),
          // Expanded detail
          if (policy.isExpanded) ...[
            Container(height: 1, color: _kBorder),
            Padding(
              padding: const EdgeInsets.all(14),
              child: _PolicyDetailFields(
                policy: policy,
                onLimitChanged: onLimitChanged,
                onWeeklyCapChanged: onWeeklyCapChanged,
                onExpiryChanged: onExpiryChanged,
              ),
            ),
          ],
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Swiss Toggle (shadcn-inspired)
// ---------------------------------------------------------------------------
class _SwissToggle extends StatelessWidget {
  final bool value;
  final ValueChanged<bool> onChanged;

  const _SwissToggle({required this.value, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: () => onChanged(!value),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        width: 40,
        height: 22,
        decoration: BoxDecoration(
          color: value ? _kActiveGreen.withValues(alpha: 0.2) : _kInactiveGray,
          borderRadius: BorderRadius.circular(11),
          border: Border.all(
            color: value ? _kActiveGreen.withValues(alpha: 0.5) : _kBorder,
          ),
        ),
        child: AnimatedAlign(
          duration: const Duration(milliseconds: 200),
          alignment: value ? Alignment.centerRight : Alignment.centerLeft,
          child: Container(
            width: 16,
            height: 16,
            margin: const EdgeInsets.symmetric(horizontal: 3),
            decoration: BoxDecoration(
              color: value ? _kActiveGreen : _kTextTertiary,
              shape: BoxShape.circle,
            ),
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Policy Detail Fields (expanded section)
// ---------------------------------------------------------------------------
class _PolicyDetailFields extends StatelessWidget {
  final _MerchantPolicy policy;
  final ValueChanged<double> onLimitChanged;
  final ValueChanged<double> onWeeklyCapChanged;
  final ValueChanged<DateTime> onExpiryChanged;

  const _PolicyDetailFields({
    required this.policy,
    required this.onLimitChanged,
    required this.onWeeklyCapChanged,
    required this.onExpiryChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // DID row
        _GridRow(
          label: 'DID',
          child: Text(
            policy.did,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 11,
              color: _kTextSecondary,
            ),
          ),
        ),
        const SizedBox(height: 12),

        // Single Transaction Limit
        _GridFieldLabel(label: 'Single Transaction Limit'),
        const SizedBox(height: 6),
        _LimitInput(
          value: policy.singleLimit,
          onChanged: onLimitChanged,
        ),
        const SizedBox(height: 14),

        // Weekly Velocity Cap
        _GridFieldLabel(label: 'Weekly Velocity Cap'),
        const SizedBox(height: 6),
        _VelocityBar(
          cap: policy.weeklyCap,
          spent: policy.weeklySpent,
          onChanged: onWeeklyCapChanged,
        ),
        const SizedBox(height: 14),

        // Expiry Date
        _GridFieldLabel(label: 'Expiry Date'),
        const SizedBox(height: 6),
        _ExpiryPicker(
          date: policy.expiry,
          onChanged: onExpiryChanged,
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Grid Row (Swiss-style key-value)
// ---------------------------------------------------------------------------
class _GridRow extends StatelessWidget {
  final String label;
  final Widget child;

  const _GridRow({required this.label, required this.child});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      decoration: BoxDecoration(
        color: _kBackground.withValues(alpha: 0.4),
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: _kBorder),
      ),
      child: Row(
        children: [
          SizedBox(
            width: 80,
            child: Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 10,
                fontWeight: FontWeight.w600,
                color: _kTextTertiary,
                letterSpacing: 0.8,
              ),
            ),
          ),
          Expanded(child: child),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Field Label
// ---------------------------------------------------------------------------
class _GridFieldLabel extends StatelessWidget {
  final String label;

  const _GridFieldLabel({required this.label});

  @override
  Widget build(BuildContext context) {
    return Text(
      label.toUpperCase(),
      style: GoogleFonts.inter(
        fontSize: 9,
        fontWeight: FontWeight.w600,
        color: _kTextTertiary,
        letterSpacing: 1.2,
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Limit Input with SOL/USD Toggle
// ---------------------------------------------------------------------------
class _LimitInput extends StatefulWidget {
  final double value;
  final ValueChanged<double> onChanged;

  const _LimitInput({required this.value, required this.onChanged});

  @override
  State<_LimitInput> createState() => _LimitInputState();
}

class _LimitInputState extends State<_LimitInput> {
  bool _showUsd = false;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 40,
      decoration: BoxDecoration(
        color: _kBackground.withValues(alpha: 0.4),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: _kBorder),
      ),
      child: Row(
        children: [
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              _showUsd
                  ? (widget.value * 157).toStringAsFixed(2)
                  : widget.value.toStringAsFixed(2),
              style: GoogleFonts.jetBrainsMono(
                fontSize: 14,
                fontWeight: FontWeight.w600,
                color: _kTextPrimary,
              ),
            ),
          ),
          // SOL / USD toggle
          GestureDetector(
            onTap: () => setState(() => _showUsd = !_showUsd),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              decoration: BoxDecoration(
                color: _kNeonCyan.withValues(alpha: 0.08),
                borderRadius: BorderRadius.circular(6),
                border: Border.all(color: _kNeonCyan.withValues(alpha: 0.2)),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    _showUsd ? 'USD' : 'SOL',
                    style: GoogleFonts.inter(
                      fontSize: 11,
                      fontWeight: FontWeight.w600,
                      color: _kNeonCyan,
                    ),
                  ),
                  const SizedBox(width: 4),
                  Icon(
                    LucideIcons.repeat2,
                    size: 11,
                    color: _kNeonCyan.withValues(alpha: 0.6),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(width: 10),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Weekly Velocity Bar
// ---------------------------------------------------------------------------
class _VelocityBar extends StatelessWidget {
  final double cap;
  final double spent;
  final ValueChanged<double> onChanged;

  const _VelocityBar({
    required this.cap,
    required this.spent,
    required this.onChanged,
  });

  double get _pct => cap > 0 ? (spent / cap).clamp(0.0, 1.0) : 0.0;

  Color get _barColor {
    if (_pct > 0.9) return _kIntercepted;
    if (_pct > 0.7) return _kPending;
    return _kNeonCyan;
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        // Progress bar
        Container(
          height: 28,
          decoration: BoxDecoration(
            color: _kBackground.withValues(alpha: 0.4),
            borderRadius: BorderRadius.circular(6),
            border: Border.all(color: _kBorder),
          ),
          child: Stack(
            children: [
              // Fill
              FractionallySizedBox(
                widthFactor: _pct,
                child: Container(
                  margin: const EdgeInsets.all(2),
                  decoration: BoxDecoration(
                    color: _barColor.withValues(alpha: 0.25),
                    borderRadius: BorderRadius.circular(4),
                  ),
                ),
              ),
              // Label
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 10),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      '${spent.toStringAsFixed(2)} SOL',
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 11,
                        fontWeight: FontWeight.w500,
                        color: _barColor,
                      ),
                    ),
                    Text(
                      '${cap.toStringAsFixed(1)} SOL',
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 11,
                        color: _kTextTertiary,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 4),
        // Percentage label
        Row(
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
              decoration: BoxDecoration(
                color: _barColor.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(4),
              ),
              child: Text(
                '${(_pct * 100).toStringAsFixed(0)}%',
                style: GoogleFonts.inter(
                  fontSize: 9,
                  fontWeight: FontWeight.w600,
                  color: _barColor,
                ),
              ),
            ),
          ],
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Expiry Date Picker
// ---------------------------------------------------------------------------
class _ExpiryPicker extends StatelessWidget {
  final DateTime date;
  final ValueChanged<DateTime> onChanged;

  const _ExpiryPicker({required this.date, required this.onChanged});

  String get _formatted {
    const months = [
      '', 'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
      'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
    ];
    return '${months[date.month]} ${date.day}, ${date.year}';
  }

  Future<void> _pickDate(BuildContext context) async {
    final picked = await showDatePicker(
      context: context,
      initialDate: date,
      firstDate: DateTime.now(),
      lastDate: DateTime.now().add(const Duration(days: 365 * 3)),
      builder: (context, child) {
        return Theme(
          data: ThemeData.dark().copyWith(
            colorScheme: const ColorScheme.dark(
              primary: _kNeonCyan,
              surface: _kSurface,
              onSurface: _kTextPrimary,
            ),
            dialogTheme: const DialogThemeData(backgroundColor: _kBackground),
          ),
          child: child!,
        );
      },
    );
    if (picked != null) onChanged(picked);
  }

  @override
  Widget build(BuildContext context) {
    final isExpired = date.isBefore(DateTime.now());
    final daysLeft = date.difference(DateTime.now()).inDays;

    return GestureDetector(
      onTap: () => _pickDate(context),
      child: Container(
        height: 40,
        decoration: BoxDecoration(
          color: _kBackground.withValues(alpha: 0.4),
          borderRadius: BorderRadius.circular(8),
          border: Border.all(
            color: isExpired
                ? _kIntercepted.withValues(alpha: 0.3)
                : _kBorder,
          ),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10),
          child: Row(
            children: [
              Icon(
                LucideIcons.calendar,
                size: 14,
                color: isExpired ? _kIntercepted : _kTextTertiary,
              ),
              const SizedBox(width: 8),
              Text(
                _formatted,
                style: GoogleFonts.inter(
                  fontSize: 13,
                  fontWeight: FontWeight.w500,
                  color: isExpired ? _kIntercepted : _kTextPrimary,
                ),
              ),
              const Spacer(),
              if (!isExpired)
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: daysLeft < 30
                        ? _kPending.withValues(alpha: 0.12)
                        : _kSuccess.withValues(alpha: 0.08),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(
                    '$daysLeft d',
                    style: GoogleFonts.jetBrainsMono(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: daysLeft < 30 ? _kPending : _kSuccess,
                    ),
                  ),
                ),
              if (isExpired)
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: _kIntercepted.withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(
                    'EXPIRED',
                    style: GoogleFonts.inter(
                      fontSize: 9,
                      fontWeight: FontWeight.w600,
                      color: _kIntercepted,
                      letterSpacing: 0.6,
                    ),
                  ),
                ),
              const SizedBox(width: 6),
              Icon(
                LucideIcons.chevronDown,
                size: 14,
                color: _kTextTertiary,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
