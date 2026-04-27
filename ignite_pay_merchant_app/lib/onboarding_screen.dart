import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';
import 'package:provider/provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

// ---------------------------------------------------------------------------
// Onboarding Screen (3-step: Welcome → Identity → Mediator)
// ---------------------------------------------------------------------------
class OnboardingScreen extends StatefulWidget {
  final VoidCallback onComplete;
  const OnboardingScreen({super.key, required this.onComplete});

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  int _step = 0;
  static const int _totalSteps = 3;

  // Step 1: DID creation
  bool _isCreating = false;
  String _generatedDid = '';

  // Step 2: Mediator config
  final _mediatorController = TextEditingController(
    text: 'wss://relay.ignite.did',
  );
  final _hubController = TextEditingController(
    text: 'http://localhost:3004',
  );
  bool _isConnecting = false;

  // Completion
  bool _complete = false;

  @override
  void dispose() {
    _mediatorController.dispose();
    _hubController.dispose();
    super.dispose();
  }

  Future<void> _createIdentity() async {
    setState(() => _isCreating = true);
    try {
      final svc = context.read<MerchantService>();
      await svc.generateIdentity();
      setState(() {
        _generatedDid = svc.did;
        _isCreating = false;
      });
    } catch (e) {
      setState(() => _isCreating = false);
      if (mounted) {
        _showError('Failed to create identity: $e');
      }
    }
  }

  Future<void> _connectAndFinish() async {
    final wsUrl = _mediatorController.text.trim();
    final hubUrl = _hubController.text.trim();

    if (wsUrl.isEmpty) {
      _showError('Mediator URL is required.');
      return;
    }
    if (!wsUrl.startsWith('ws://') && !wsUrl.startsWith('wss://')) {
      _showError('URL must start with ws:// or wss://');
      return;
    }

    setState(() => _isConnecting = true);

    try {
      final svc = context.read<MerchantService>();
      await svc.saveConfig(hubUrl, wsUrl);

      // Initialize push notifications
      if (wsUrl.isNotEmpty) {
        try {
          final pushSvc = context.read<MerchantPushService>();
          await pushSvc.initialize();
          await pushSvc.connectToMediator(wsUrl);
        } catch (_) {
          // Non-fatal: push service will be retried on next app launch
        }
      }

      // Save hub registry URL
      if (hubUrl.isNotEmpty) {
        final prefs = await SharedPreferences.getInstance();
        await prefs.setString('hub_registry_url', hubUrl);
      }

      if (mounted) setState(() { _isConnecting = false; _complete = true; });
    } catch (e) {
      if (mounted) {
        setState(() => _isConnecting = false);
        _showError('Connection failed: $e');
      }
    }
  }

  void _showError(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        backgroundColor: kDanger,
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
        margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
        content: Text(message, style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        duration: const Duration(seconds: 4),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (_complete) {
      return Scaffold(
        backgroundColor: kBackground,
        body: SafeArea(
          child: Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 72,
                  height: 72,
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(18),
                    color: kSuccess.withValues(alpha: 0.12),
                    border: Border.all(color: kSuccess.withValues(alpha: 0.3)),
                  ),
                  child: Icon(LucideIcons.check, size: 36, color: kSuccess),
                ),
                const SizedBox(height: 24),
                Text(
                  "You're all set!",
                  style: GoogleFonts.inter(
                    fontSize: 24,
                    fontWeight: FontWeight.w700,
                    color: kTextPrimary,
                  ),
                ),
                const SizedBox(height: 10),
                Text(
                  'Scan a merchant MCP QR code to pair\nvia Settings → Connections.',
                  textAlign: TextAlign.center,
                  style: GoogleFonts.inter(
                    fontSize: 13,
                    color: kTextSecondary,
                    height: 1.5,
                  ),
                ),
                const SizedBox(height: 32),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 40),
                  child: _OnboardingButton(
                    label: 'Enter Ignite Merchant',
                    onTap: widget.onComplete,
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    }

    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 24),
          child: Column(
            children: [
              const SizedBox(height: 40),
              _StepProgress(current: _step, total: _totalSteps),
              const SizedBox(height: 40),
              Expanded(
                child: AnimatedSwitcher(
                  duration: const Duration(milliseconds: 350),
                  child: switch (_step) {
                    0 => _WelcomeStep(
                        key: const ValueKey('welcome'),
                        onNext: () => setState(() => _step = 1),
                      ),
                    1 => _CreateIdentityStep(
                        key: const ValueKey('identity'),
                        isCreating: _isCreating,
                        generatedDid: _generatedDid,
                        onCreate: _createIdentity,
                        onNext: () => setState(() => _step = 2),
                      ),
                    2 => _MediatorConfigStep(
                        key: const ValueKey('mediator'),
                        mediatorController: _mediatorController,
                        hubController: _hubController,
                        isConnecting: _isConnecting,
                        onConnect: _connectAndFinish,
                      ),
                    _ => const SizedBox.shrink(),
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Step Progress Indicator
// ---------------------------------------------------------------------------
class _StepProgress extends StatelessWidget {
  final int current;
  final int total;

  const _StepProgress({required this.current, required this.total});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: List.generate(total, (i) {
        final isActive = i <= current;
        final isCurrent = i == current;
        return Expanded(
          child: Container(
            height: 3,
            margin: EdgeInsets.only(left: i > 0 ? 4 : 0),
            decoration: BoxDecoration(
              color: isActive ? kNeonCyan : kSurfaceMid,
              borderRadius: BorderRadius.circular(2),
              boxShadow: isCurrent
                  ? [
                      BoxShadow(
                        color: kNeonCyan.withValues(alpha: 0.4),
                        blurRadius: 6,
                      ),
                    ]
                  : null,
            ),
          ),
        );
      }),
    );
  }
}

// ---------------------------------------------------------------------------
// Step 0: Welcome
// ---------------------------------------------------------------------------
class _WelcomeStep extends StatelessWidget {
  final VoidCallback onNext;
  const _WelcomeStep({super.key, required this.onNext});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        const Spacer(flex: 2),
        // Logo
        Container(
          width: 80,
          height: 80,
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(20),
            gradient: const LinearGradient(
              colors: [kNeonCyan, kNeonCyanDim],
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
            ),
            boxShadow: [
              BoxShadow(
                color: kNeonCyan.withValues(alpha: 0.3),
                blurRadius: 30,
                spreadRadius: 4,
              ),
            ],
          ),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(20),
            child: Image.asset('assets/icons/ignite_pay_merchant.png',
                width: 80, height: 80, fit: BoxFit.cover),
          ),
        ),
        const SizedBox(height: 28),
        Text(
          'Ignite Merchant',
          style: GoogleFonts.inter(
            fontSize: 32,
            fontWeight: FontWeight.w800,
            color: kTextPrimary,
            letterSpacing: -1,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          'Your AI Payment Merchant Hub',
          style: GoogleFonts.inter(
            fontSize: 15,
            color: kTextSecondary,
          ),
        ),
        const SizedBox(height: 32),
        _FeatureItem(
          icon: LucideIcons.fingerprint,
          text: 'DID identity (did:ignite)',
        ),
        const SizedBox(height: 10),
        _FeatureItem(
          icon: LucideIcons.messageSquare,
          text: 'End-to-end DIDComm encryption',
        ),
        const SizedBox(height: 10),
        _FeatureItem(
          icon: LucideIcons.zap,
          text: 'State channel payments',
        ),
        const SizedBox(height: 10),
        _FeatureItem(
          icon: LucideIcons.scanLine,
          text: 'QR payment collection',
        ),
        const Spacer(flex: 3),
        _OnboardingButton(label: 'Get Started', onTap: onNext),
        const SizedBox(height: 40),
      ],
    );
  }
}

class _FeatureItem extends StatelessWidget {
  final IconData icon;
  final String text;
  const _FeatureItem({required this.icon, required this.text});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(icon, size: 16, color: kNeonCyan.withValues(alpha: 0.8)),
        const SizedBox(width: 10),
        Expanded(
          child: Text(
            text,
            style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary),
          ),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Step 1: Create Identity
// ---------------------------------------------------------------------------
class _CreateIdentityStep extends StatelessWidget {
  final bool isCreating;
  final String generatedDid;
  final VoidCallback onCreate;
  final VoidCallback onNext;

  const _CreateIdentityStep({
    super.key,
    required this.isCreating,
    required this.generatedDid,
    required this.onCreate,
    required this.onNext,
  });

  @override
  Widget build(BuildContext context) {
    final hasDid = generatedDid.isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        const Spacer(flex: 1),
        Container(
          width: 64,
          height: 64,
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(16),
            color: kPurple.withValues(alpha: 0.15),
            border: Border.all(color: kPurple.withValues(alpha: 0.3)),
          ),
          child: Icon(LucideIcons.keyRound, size: 28, color: kPurple),
        ),
        const SizedBox(height: 20),
        Text(
          'Create Merchant Identity',
          style: GoogleFonts.inter(
            fontSize: 22,
            fontWeight: FontWeight.w700,
            color: kTextPrimary,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          'Generate a unique did:ignite identity\nfor your merchant hub.',
          textAlign: TextAlign.center,
          style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary, height: 1.5),
        ),
        const SizedBox(height: 28),
        if (!hasDid && !isCreating) ...[
          _OnboardingButton(
            label: 'Generate DID',
            onTap: onCreate,
          ),
        ],
        if (isCreating) ...[
          const SizedBox(height: 20),
          CircularProgressIndicator(
            color: kNeonCyan.withValues(alpha: 0.7),
            strokeWidth: 2,
          ),
          const SizedBox(height: 12),
          Text(
            'Generating Ed25519 keypair...',
            style: GoogleFonts.inter(fontSize: 12, color: kTextTertiary),
          ),
        ],
        if (hasDid) ...[
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(
              color: kSurfaceDark,
              borderRadius: BorderRadius.circular(14),
              border: Border.all(color: kSuccess.withValues(alpha: 0.25)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(LucideIcons.checkCircle2, size: 16, color: kSuccess),
                    const SizedBox(width: 8),
                    Text(
                      'IDENTITY CREATED',
                      style: GoogleFonts.inter(
                        fontSize: 10,
                        fontWeight: FontWeight.w700,
                        color: kSuccess,
                        letterSpacing: 1.0,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 10),
                Row(
                  children: [
                    Expanded(
                      child: SelectableText(
                        generatedDid,
                        style: GoogleFonts.jetBrainsMono(
                          fontSize: 12,
                          color: kTextPrimary,
                        ),
                      ),
                    ),
                    GestureDetector(
                      onTap: () => Clipboard.setData(ClipboardData(text: generatedDid)),
                      child: const Icon(LucideIcons.copy, size: 14, color: kTextSecondary),
                    ),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(height: 20),
          _OnboardingButton(label: 'Continue', onTap: onNext),
        ],
        const Spacer(flex: 2),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Step 2: Connect Mediator
// ---------------------------------------------------------------------------
class _MediatorConfigStep extends StatelessWidget {
  final TextEditingController mediatorController;
  final TextEditingController hubController;
  final bool isConnecting;
  final VoidCallback onConnect;

  const _MediatorConfigStep({
    super.key,
    required this.mediatorController,
    required this.hubController,
    required this.isConnecting,
    required this.onConnect,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        const Spacer(flex: 1),
        Container(
          width: 64,
          height: 64,
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(16),
            color: kCyan.withValues(alpha: 0.15),
            border: Border.all(color: kCyan.withValues(alpha: 0.3)),
          ),
          child: Icon(LucideIcons.radio, size: 28, color: kCyan),
        ),
        const SizedBox(height: 20),
        Text(
          'Connect to Mediator',
          style: GoogleFonts.inter(
            fontSize: 22,
            fontWeight: FontWeight.w700,
            color: kTextPrimary,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          'The Mediator routes encrypted DIDComm\nmessages between you and MCP agents.',
          textAlign: TextAlign.center,
          style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary, height: 1.5),
        ),
        const SizedBox(height: 28),
        // Mediator WS URL input
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 14),
          decoration: BoxDecoration(
            color: kSurfaceMid,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(color: kBorder),
          ),
          child: TextField(
            controller: mediatorController,
            enabled: !isConnecting,
            style: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextPrimary),
            decoration: InputDecoration(
              border: InputBorder.none,
              hintText: 'wss://relay.ignite.did',
              hintStyle: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextTertiary),
              isDense: true,
              contentPadding: const EdgeInsets.symmetric(vertical: 12),
            ),
          ),
        ),
        const SizedBox(height: 16),
        // Hub Registry URL input
        Text(
          'Hub Registry URL',
          style: GoogleFonts.inter(
            fontSize: 13,
            fontWeight: FontWeight.w600,
            color: kTextSecondary,
          ),
        ),
        const SizedBox(height: 6),
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 14),
          decoration: BoxDecoration(
            color: kSurfaceMid,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(color: kBorder),
          ),
          child: TextField(
            controller: hubController,
            enabled: !isConnecting,
            style: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextPrimary),
            decoration: InputDecoration(
              border: InputBorder.none,
              hintText: 'http://localhost:3004',
              hintStyle: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextTertiary),
              isDense: true,
              contentPadding: const EdgeInsets.symmetric(vertical: 12),
            ),
          ),
        ),
        const SizedBox(height: 20),
        if (isConnecting)
          Column(
            children: [
              CircularProgressIndicator(
                color: kNeonCyan.withValues(alpha: 0.7),
                strokeWidth: 2,
              ),
              const SizedBox(height: 12),
              Text(
                'Connecting to mediator...',
                style: GoogleFonts.inter(fontSize: 12, color: kTextTertiary),
              ),
            ],
          )
        else
          _OnboardingButton(label: 'Connect & Continue', onTap: onConnect),
        const Spacer(flex: 2),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Shared Button
// ---------------------------------------------------------------------------
class _OnboardingButton extends StatelessWidget {
  final String label;
  final VoidCallback onTap;

  const _OnboardingButton({required this.label, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: double.infinity,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 14),
          decoration: BoxDecoration(
            gradient: const LinearGradient(
              colors: [kNeonCyan, kNeonCyanDim],
            ),
            borderRadius: BorderRadius.circular(12),
            boxShadow: [
              BoxShadow(
                color: kNeonCyan.withValues(alpha: 0.25),
                blurRadius: 16,
                spreadRadius: 2,
              ),
            ],
          ),
          child: Center(
            child: Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 14,
                fontWeight: FontWeight.w700,
                color: kBackground,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
