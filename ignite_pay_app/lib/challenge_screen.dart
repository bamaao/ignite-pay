import 'dart:ui';
import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';

// ---------------------------------------------------------------------------
// Challenge Theme
// ---------------------------------------------------------------------------
const _kAmber = Color(0xFFFFB300);
const _kAmberDim = Color(0xFF9E7700);
const _kAmberGlow = Color(0x33FFB300);
const _kBackground = Color(0xFF0F0F1A);
const _kSurfaceDark = Color(0xFF1A1A2E);
const _kSurfaceMid = Color(0xFF16213E);
const _kTextPrimary = Color(0xFFE8E8F0);
const _kTextSecondary = Color(0xFF8A8AA0);
const _kDanger = Color(0xFFFF5252);
const _kGlassBorder = Color(0x1AFFFFFF);
const _kSuccess = Color(0xFF00E676);

// ---------------------------------------------------------------------------
// Challenge Overlay Entry Point
// ---------------------------------------------------------------------------
Future<T?> showX402Challenge<T>(BuildContext context, {AuthRequest? request}) {
  return Navigator.of(context).push<T>(
    PageRouteBuilder(
      opaque: false,
      fullscreenDialog: true,
      transitionDuration: const Duration(milliseconds: 400),
      reverseTransitionDuration: const Duration(milliseconds: 300),
      pageBuilder: (_, animation, _) {
        return AnimatedBuilder(
          animation: animation,
          builder: (context, child) {
            return FadeTransition(
              opacity: CurvedAnimation(
                parent: animation,
                curve: Curves.easeOut,
              ),
              child: child,
            );
          },
          child: _X402ChallengeScreen(request: request),
        );
      },
    ),
  );
}

// ---------------------------------------------------------------------------
// Challenge Screen
// ---------------------------------------------------------------------------
class _X402ChallengeScreen extends StatefulWidget {
  final AuthRequest? request;

  const _X402ChallengeScreen({this.request});

  @override
  State<_X402ChallengeScreen> createState() => _X402ChallengeScreenState();
}

class _X402ChallengeScreenState extends State<_X402ChallengeScreen>
    with SingleTickerProviderStateMixin {
  late final AnimationController _glowCtrl;
  String _authResult = '';
  bool _isAuthorizing = false;
  String _listAction = 'none';
  String _listLabel = '';
  String _listMaxAmount = '';

  String get _merchantDid => widget.request?.merchantDid ?? 'did:solana:shopx merchants';
  int get _amount => widget.request?.amount ?? 500000000;
  String get _paymentId => widget.request?.paymentId ?? '';
  String get _description => widget.request?.description ?? 'Payment for services';

  bool get _showLabelInput => _listAction == 'add_whitelist' || _listAction == 'add_blacklist';
  bool get _showMaxAmountInput => _listAction == 'add_whitelist';

  @override
  void initState() {
    super.initState();
    _glowCtrl = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 2000),
    )..repeat(reverse: true);
  }

  @override
  void dispose() {
    _glowCtrl.dispose();
    super.dispose();
  }

  Future<void> _onAuthorize() async {
    setState(() {
      _isAuthorizing = true;
      _authResult = 'Creating session key...';
    });
    try {
      // V2.0: Create a session key for this payment authorization.
      // Spending limit is set to 10x the payment amount to allow multiple payments.
      // Duration is 1 hour (3600 seconds).
      final spendingLimit = _amount * 10;
      final durationSecs = 3600;

      // Send auth response with session key data via DidcommService
      await DidcommService().sendAuthResponseWithSessionKey(
        paymentId: _paymentId,
        authorized: true,
        listAction: _listAction,
        spendingLimit: spendingLimit,
        durationSecs: durationSecs,
        listLabel: _showLabelInput && _listLabel.isNotEmpty ? _listLabel : null,
        listMaxAmount: _showMaxAmountInput && _listMaxAmount.isNotEmpty
            ? int.tryParse(_listMaxAmount)
            : null,
      );

      setState(() {
        _authResult = 'Authorized with session key';
      });
      await Future.delayed(const Duration(milliseconds: 1200));
      if (mounted) Navigator.of(context).pop('authorized');
    } catch (e) {
      setState(() {
        _authResult = 'Error: $e';
        _isAuthorizing = false;
      });
    }
  }

  void _onDecline() {
    Navigator.of(context).pop('declined');
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.transparent,
      body: Stack(
        children: [
          // Blurred backdrop
          BackdropFilter(
            filter: ImageFilter.blur(sigmaX: 20, sigmaY: 20),
            child: Container(
              color: _kBackground.withValues(alpha: 0.85),
            ),
          ),

          // Ambient amber glow
          Positioned(
            top: -80,
            left: -80,
            right: -80,
            height: 300,
            child: AnimatedBuilder(
              animation: _glowCtrl,
              builder: (context, _) {
                return Container(
                  decoration: BoxDecoration(
                    gradient: RadialGradient(
                      center: Alignment.topCenter,
                      radius: 0.8,
                      colors: [
                        _kAmberGlow.withValues(alpha: 0.35 + 0.1 * _glowCtrl.value),
                        Colors.transparent,
                      ],
                    ),
                  ),
                );
              },
            ),
          ),

          // Main content
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 24),
              child: Column(
                children: [
                  const SizedBox(height: 16),
                  const _ChallengeHeader(),
                  const Spacer(flex: 1),
                  _MerchantCard(merchantDid: _merchantDid),
                  const SizedBox(height: 28),
                  _AmountDisplay(amount: _amount),
                  const SizedBox(height: 20),
                  _ReasonBlock(description: _description),
                  const SizedBox(height: 16),
                  _ListActionSelector(
                    selected: _listAction,
                    onChanged: (v) => setState(() => _listAction = v),
                    label: _listLabel,
                    onLabelChanged: (v) => setState(() => _listLabel = v),
                    maxAmount: _listMaxAmount,
                    onMaxAmountChanged: (v) => setState(() => _listMaxAmount = v),
                    showLabelInput: _showLabelInput,
                    showMaxAmountInput: _showMaxAmountInput,
                  ),
                  const Spacer(flex: 2),
                  if (_authResult.isNotEmpty) ...[
                    _ResultBanner(result: _authResult),
                    const SizedBox(height: 16),
                  ],
                  SlideToAuthorize(
                    onAuthorized: _onAuthorize,
                    enabled: !_isAuthorizing,
                  ),
                  const SizedBox(height: 12),
                  _DeclineButton(onTap: _onDecline),
                  const SizedBox(height: 32),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Header Bar
// ---------------------------------------------------------------------------
class _ChallengeHeader extends StatelessWidget {
  const _ChallengeHeader();

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Row(
          children: [
            Container(
              width: 32,
              height: 32,
              decoration: BoxDecoration(
                color: _kAmber.withValues(alpha: 0.15),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: _kAmber.withValues(alpha: 0.3)),
              ),
              child: const Icon(LucideIcons.shieldAlert, size: 18, color: _kAmber),
            ),
            const SizedBox(width: 10),
            Text(
              'X402 Challenge',
              style: GoogleFonts.inter(
                fontSize: 16,
                fontWeight: FontWeight.w700,
                color: _kTextPrimary,
              ),
            ),
          ],
        ),
        GestureDetector(
          onTap: () => Navigator.of(context).pop('dismissed'),
          child: Container(
            width: 32,
            height: 32,
            decoration: BoxDecoration(
              color: _kSurfaceMid.withValues(alpha: 0.5),
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: _kGlassBorder),
            ),
            child: const Icon(LucideIcons.x, size: 18, color: _kTextSecondary),
          ),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Merchant Profile Card
// ---------------------------------------------------------------------------
class _MerchantCard extends StatelessWidget {
  final String? merchantDid;
  const _MerchantCard({this.merchantDid});

  String get _displayDid {
    if (merchantDid != null && merchantDid!.isNotEmpty) {
      if (merchantDid!.length > 24) {
        return '${merchantDid!.substring(0, 16)}...${merchantDid!.substring(merchantDid!.length - 6)}';
      }
      return merchantDid!;
    }
    return 'did:solana:7kPx...mN3q';
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(18),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.7),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: _kGlassBorder),
      ),
      child: Row(
        children: [
          // Avatar
          Container(
            width: 52,
            height: 52,
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(14),
              gradient: const LinearGradient(
                colors: [Color(0xFF6C5CE7), Color(0xFFA29BFE)],
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
              ),
            ),
            child: const Icon(
              LucideIcons.store,
              size: 24,
              color: Colors.white,
            ),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Text(
                      'Merchant',
                      style: GoogleFonts.inter(
                        fontSize: 16,
                        fontWeight: FontWeight.w600,
                        color: _kTextPrimary,
                      ),
                    ),
                    const SizedBox(width: 8),
                    // Verified badge
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                      decoration: BoxDecoration(
                        color: _kSuccess.withValues(alpha: 0.12),
                        borderRadius: BorderRadius.circular(6),
                        border: Border.all(color: _kSuccess.withValues(alpha: 0.3)),
                      ),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          const Icon(LucideIcons.badgeCheck, size: 11, color: _kSuccess),
                          const SizedBox(width: 3),
                          Text(
                            'Verified',
                            style: GoogleFonts.inter(
                              fontSize: 9,
                              fontWeight: FontWeight.w600,
                              color: _kSuccess,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 4),
                Text(
                  _displayDid,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11,
                    color: _kTextSecondary,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ],
            ),
          ),
          const SizedBox(width: 8),
          Icon(
            LucideIcons.chevronRight,
            size: 20,
            color: _kTextSecondary.withValues(alpha: 0.5),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Amount Display
// ---------------------------------------------------------------------------
class _AmountDisplay extends StatelessWidget {
  final int amount;
  const _AmountDisplay({required this.amount});

  /// Convert lamports to SOL.
  String get _solAmount {
    final sol = amount / 1000000000.0;
    if (sol >= 1.0) {
      return sol.toStringAsFixed(sol.truncateToDouble() == sol ? 0 : 2);
    }
    return sol.toStringAsFixed(4).replaceAll(RegExp(r'0+$'), '').replaceAll(RegExp(r'\.$'), '');
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Text(
          'PAYMENT REQUEST',
          style: GoogleFonts.inter(
            fontSize: 11,
            fontWeight: FontWeight.w600,
            color: _kTextSecondary,
            letterSpacing: 1.5,
          ),
        ),
        const SizedBox(height: 8),
        Row(
          mainAxisAlignment: MainAxisAlignment.center,
          crossAxisAlignment: CrossAxisAlignment.baseline,
          textBaseline: TextBaseline.alphabetic,
          children: [
            Text(
              _solAmount,
              style: GoogleFonts.inter(
                fontSize: 52,
                fontWeight: FontWeight.w800,
                color: _kTextPrimary,
                height: 1.0,
              ),
            ),
            const SizedBox(width: 6),
            Text(
              'SOL',
              style: GoogleFonts.inter(
                fontSize: 20,
                fontWeight: FontWeight.w600,
                color: _kAmber,
              ),
            ),
          ],
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Reason Block
// ---------------------------------------------------------------------------
class _ReasonBlock extends StatelessWidget {
  final String description;
  const _ReasonBlock({required this.description});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: _kSurfaceMid.withValues(alpha: 0.4),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: _kGlassBorder),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 32,
            height: 32,
            decoration: BoxDecoration(
              color: _kAmber.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(8),
            ),
            child: const Icon(LucideIcons.fileText, size: 16, color: _kAmber),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Reason',
                  style: GoogleFonts.inter(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: _kTextSecondary,
                    letterSpacing: 0.8,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  description.isNotEmpty ? description : 'No description provided',
                  style: GoogleFonts.inter(
                    fontSize: 13,
                    color: _kTextPrimary.withValues(alpha: 0.85),
                    height: 1.4,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Result Banner
// ---------------------------------------------------------------------------
class _ResultBanner extends StatelessWidget {
  final String result;

  const _ResultBanner({required this.result});

  bool get _isError => result.startsWith('Error');

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: (_isError ? _kDanger : _kSuccess).withValues(alpha: 0.1),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(
          color: (_isError ? _kDanger : _kSuccess).withValues(alpha: 0.25),
        ),
      ),
      child: Row(
        children: [
          Icon(
            _isError ? LucideIcons.xCircle : LucideIcons.checkCircle2,
            size: 16,
            color: _isError ? _kDanger : _kSuccess,
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              result,
              style: GoogleFonts.jetBrainsMono(
                fontSize: 11,
                color: _isError ? _kDanger : _kSuccess,
              ),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Slide-to-Authorize Slider (Apple-style)
// ---------------------------------------------------------------------------
class SlideToAuthorize extends StatefulWidget {
  final VoidCallback onAuthorized;
  final bool enabled;

  const SlideToAuthorize({
    super.key,
    required this.onAuthorized,
    this.enabled = true,
  });

  @override
  State<SlideToAuthorize> createState() => _SlideToAuthorizeState();
}

class _SlideToAuthorizeState extends State<SlideToAuthorize>
    with SingleTickerProviderStateMixin {
  double _dragPosition = 0;
  bool _authorized = false;
  late final AnimationController _resetCtrl;
  late Animation<double> _resetAnim;

  @override
  void initState() {
    super.initState();
    _resetCtrl = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 400),
    );
    _resetAnim = Tween<double>(begin: 0, end: 0).animate(
      CurvedAnimation(parent: _resetCtrl, curve: Curves.easeOutCubic),
    );
    _resetCtrl.addListener(() {
      if (mounted) setState(() => _dragPosition = _resetAnim.value);
    });
  }

  @override
  void dispose() {
    _resetCtrl.dispose();
    super.dispose();
  }

  void _onDragEnd(DragEndDetails details) {
    if (_authorized) return;

    final box = context.findRenderObject() as RenderBox;
    final maxDrag = box.size.width - 56 - 12; // thumb width + padding

    if (_dragPosition > maxDrag * 0.85) {
      setState(() => _authorized = true);
      widget.onAuthorized();
    } else {
      // Animate back to start
      _resetAnim = Tween<double>(
        begin: _dragPosition,
        end: 0,
      ).animate(CurvedAnimation(parent: _resetCtrl, curve: Curves.easeOutCubic));

      _resetCtrl.forward(from: 0).then((_) {
        if (mounted) setState(() => _dragPosition = 0);
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final maxWidth = constraints.maxWidth;
        final thumbSize = 50.0;
        final horizontalPadding = 6.0;
        final maxDrag = maxWidth - thumbSize - horizontalPadding * 2;
        final progress = (_dragPosition / maxDrag).clamp(0.0, 1.0);

        return Container(
          height: 62,
          decoration: BoxDecoration(
            color: _authorized
                ? _kSuccess.withValues(alpha: 0.2)
                : _kAmber.withValues(alpha: 0.08),
            borderRadius: BorderRadius.circular(31),
            border: Border.all(
              color: _authorized
                  ? _kSuccess.withValues(alpha: 0.4)
                  : _kAmber.withValues(alpha: 0.2),
            ),
          ),
          child: Stack(
            children: [
              // Track fill
              if (!_authorized)
                Positioned(
                  left: 0,
                  top: 0,
                  bottom: 0,
                  child: Container(
                    width: horizontalPadding + _dragPosition + thumbSize / 2,
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        colors: [
                          _kAmber.withValues(alpha: 0.15),
                          _kAmber.withValues(alpha: 0.05),
                        ],
                      ),
                      borderRadius: BorderRadius.circular(31),
                    ),
                  ),
                ),

              // Label text
              if (!_authorized)
                Center(
                  child: Opacity(
                    opacity: 1 - progress * 0.8,
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(
                          LucideIcons.arrowRight,
                          size: 16,
                          color: _kAmber.withValues(alpha: 0.6),
                        ),
                        const SizedBox(width: 8),
                        Text(
                          'Slide to Authorize',
                          style: GoogleFonts.inter(
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                            color: _kAmber.withValues(alpha: 0.7),
                            letterSpacing: 0.5,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),

              // Authorized state
              if (_authorized)
                Center(
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const Icon(LucideIcons.check, size: 18, color: _kSuccess),
                      const SizedBox(width: 8),
                      Text(
                        'Authorized',
                        style: GoogleFonts.inter(
                          fontSize: 14,
                          fontWeight: FontWeight.w600,
                          color: _kSuccess,
                        ),
                      ),
                    ],
                  ),
                ),

              // Draggable thumb
              if (!_authorized)
                Positioned(
                  left: horizontalPadding + _dragPosition,
                  top: 6,
                  child: GestureDetector(
                    onHorizontalDragUpdate: widget.enabled
                        ? (details) {
                            setState(() {
                              _dragPosition = (
                                _dragPosition + details.delta.dx
                              ).clamp(0.0, maxDrag);
                            });
                          }
                        : null,
                    onHorizontalDragEnd: widget.enabled ? _onDragEnd : null,
                    child: Container(
                      width: thumbSize,
                      height: thumbSize,
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          colors: [
                            _kAmber,
                            _kAmberDim,
                          ],
                          begin: Alignment.topLeft,
                          end: Alignment.bottomRight,
                        ),
                        shape: BoxShape.circle,
                        boxShadow: [
                          BoxShadow(
                            color: _kAmber.withValues(alpha: 0.3 + 0.2 * progress),
                            blurRadius: 12,
                            spreadRadius: 1,
                          ),
                        ],
                      ),
                      child: Icon(
                        LucideIcons.chevronRight,
                        size: 22,
                        color: _kBackground,
                      ),
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
// Decline & Block Button (Ghost style)
// ---------------------------------------------------------------------------
class _DeclineButton extends StatelessWidget {
  final VoidCallback onTap;

  const _DeclineButton({required this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: double.infinity,
        height: 48,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(24),
          border: Border.all(
            color: _kDanger.withValues(alpha: 0.25),
          ),
          color: _kDanger.withValues(alpha: 0.05),
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              LucideIcons.shieldOff,
              size: 16,
              color: _kDanger.withValues(alpha: 0.8),
            ),
            const SizedBox(width: 8),
            Text(
              'Decline & Block',
              style: GoogleFonts.inter(
                fontSize: 14,
                fontWeight: FontWeight.w600,
                color: _kDanger.withValues(alpha: 0.85),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// List Action Selector (V1.1 — extended with 6 actions + label/max_amount inputs)
// ---------------------------------------------------------------------------
class _ListActionSelector extends StatelessWidget {
  final String selected;
  final ValueChanged<String> onChanged;
  final String label;
  final ValueChanged<String> onLabelChanged;
  final String maxAmount;
  final ValueChanged<String> onMaxAmountChanged;
  final bool showLabelInput;
  final bool showMaxAmountInput;

  const _ListActionSelector({
    required this.selected,
    required this.onChanged,
    required this.label,
    required this.onLabelChanged,
    required this.maxAmount,
    required this.onMaxAmountChanged,
    required this.showLabelInput,
    required this.showMaxAmountInput,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: _kGlassBorder),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'LIST ACTION',
            style: GoogleFonts.inter(
              fontSize: 10,
              fontWeight: FontWeight.w600,
              color: _kTextSecondary,
              letterSpacing: 1.0,
            ),
          ),
          const SizedBox(height: 10),
          // Row 1: This time only + Add Whitelist + Add Blacklist
          Row(
            children: [
              _ListActionChip(
                label: 'This time only',
                value: 'none',
                selectedValue: selected,
                onTap: () => onChanged('none'),
              ),
              const SizedBox(width: 6),
              _ListActionChip(
                label: 'Whitelist',
                value: 'add_whitelist',
                selectedValue: selected,
                color: _kSuccess,
                onTap: () => onChanged('add_whitelist'),
              ),
              const SizedBox(width: 6),
              _ListActionChip(
                label: 'Blacklist',
                value: 'add_blacklist',
                selectedValue: selected,
                color: _kDanger,
                onTap: () => onChanged('add_blacklist'),
              ),
            ],
          ),
          const SizedBox(height: 8),
          // Row 2: Remove Whitelist + Remove Blacklist
          Row(
            children: [
              _ListActionChip(
                label: 'Remove WL',
                value: 'remove_whitelist',
                selectedValue: selected,
                color: _kSuccess.withValues(alpha: 0.6),
                onTap: () => onChanged('remove_whitelist'),
              ),
              const SizedBox(width: 6),
              _ListActionChip(
                label: 'Remove BL',
                value: 'remove_blacklist',
                selectedValue: selected,
                color: _kDanger.withValues(alpha: 0.6),
                onTap: () => onChanged('remove_blacklist'),
              ),
            ],
          ),
          // V1.1: Label input (shown for add_whitelist and add_blacklist)
          if (showLabelInput) ...[
            const SizedBox(height: 10),
            _LabelInputField(
              value: label,
              onChanged: onLabelChanged,
              hint: 'Label (e.g. "ShopX Marketplace")',
            ),
          ],
          // V1.1: Max amount input (shown for add_whitelist only)
          if (showMaxAmountInput) ...[
            const SizedBox(height: 8),
            _LabelInputField(
              value: maxAmount,
              onChanged: onMaxAmountChanged,
              hint: 'Max amount (lamports, optional)',
              keyboardType: TextInputType.number,
            ),
          ],
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Text input field for label and max amount
// ---------------------------------------------------------------------------
class _LabelInputField extends StatelessWidget {
  final String value;
  final ValueChanged<String> onChanged;
  final String hint;
  final TextInputType? keyboardType;

  const _LabelInputField({
    required this.value,
    required this.onChanged,
    required this.hint,
    this.keyboardType,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 40,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      decoration: BoxDecoration(
        color: _kSurfaceMid.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: _kGlassBorder),
      ),
      child: TextField(
        onChanged: onChanged,
        controller: TextEditingController(text: value)..selection = TextSelection.collapsed(offset: value.length),
        style: GoogleFonts.inter(
          fontSize: 12,
          color: _kTextPrimary,
        ),
        decoration: InputDecoration(
          border: InputBorder.none,
          hintText: hint,
          hintStyle: GoogleFonts.inter(
            fontSize: 12,
            color: _kTextSecondary.withValues(alpha: 0.5),
          ),
          isDense: true,
          contentPadding: const EdgeInsets.only(top: 10),
        ),
        keyboardType: keyboardType,
      ),
    );
  }
}

class _ListActionChip extends StatelessWidget {
  final String label;
  final String value;
  final String selectedValue;
  final Color color;
  final VoidCallback onTap;

  const _ListActionChip({
    required this.label,
    required this.value,
    required this.selectedValue,
    required this.onTap,
    this.color = _kAmber,
  });

  bool get _isSelected => value == selectedValue;

  @override
  Widget build(BuildContext context) {
    return Expanded(
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 8),
          decoration: BoxDecoration(
            color: _isSelected ? color.withValues(alpha: 0.15) : _kSurfaceMid.withValues(alpha: 0.4),
            borderRadius: BorderRadius.circular(8),
            border: Border.all(
              color: _isSelected ? color.withValues(alpha: 0.5) : _kGlassBorder,
            ),
          ),
          child: Center(
            child: Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: _isSelected ? color : _kTextSecondary,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
