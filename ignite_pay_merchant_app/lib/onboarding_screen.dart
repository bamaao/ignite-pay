import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';
import 'package:provider/provider.dart';

class OnboardingScreen extends StatefulWidget {
  final VoidCallback onComplete;
  const OnboardingScreen({super.key, required this.onComplete});

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  final _hubController = TextEditingController();
  final _mediatorController = TextEditingController();
  bool _generating = false;
  bool _identityReady = false;

  @override
  void dispose() {
    _hubController.dispose();
    _mediatorController.dispose();
    super.dispose();
  }

  Future<void> _generateIdentity() async {
    setState(() => _generating = true);
    try {
      final svc = context.read<MerchantService>();
      await svc.generateIdentity();
      setState(() => _identityReady = true);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          backgroundColor: kDanger,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text('身份生成失败: $e', style: GoogleFonts.inter(fontWeight: FontWeight.w600)),
        ));
      }
    } finally {
      setState(() => _generating = false);
    }
  }

  Future<void> _start() async {
    final svc = context.read<MerchantService>();
    await svc.saveConfig(_hubController.text, _mediatorController.text);

    // Initialize push notifications if mediator URL is configured
    if (_mediatorController.text.isNotEmpty) {
      try {
        final pushSvc = context.read<MerchantPushService>();
        await pushSvc.initialize();
        await pushSvc.connectToMediator(_mediatorController.text);
      } catch (e) {
        // Non-fatal: push service will be retried on next app launch
      }
    }

    widget.onComplete();
  }

  @override
  Widget build(BuildContext context) {
    final svc = context.watch<MerchantService>();

    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 40),
          child: SingleChildScrollView(
            child: Column(
              children: [
                // Logo
                Container(
                  width: 64,
                  height: 64,
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(16),
                    gradient: const LinearGradient(
                      colors: [kNeonCyan, kNeonCyanDim],
                      begin: Alignment.topLeft,
                      end: Alignment.bottomRight,
                    ),
                  ),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(16),
                    child: Image.asset('assets/icons/ignite_pay_merchant.png', width: 64, height: 64, fit: BoxFit.cover),
                  ),
                ),
                const SizedBox(height: 16),
                Text('Ignite Merchant',
                    style: GoogleFonts.inter(
                      fontSize: 24,
                      fontWeight: FontWeight.w700,
                      color: kTextPrimary,
                    )),
                const SizedBox(height: 6),
                Text('首次配置',
                    style: GoogleFonts.inter(fontSize: 14, color: kTextSecondary)),
                const SizedBox(height: 32),

                // Hub Endpoint
                const SectionLabel(text: 'HUB ENDPOINT'),
                const SizedBox(height: 8),
                TextField(
                  controller: _hubController,
                  style: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextPrimary),
                  decoration: InputDecoration(
                    hintText: 'https://hub.example.com',
                    hintStyle: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextTertiary),
                    filled: true,
                    fillColor: kSurfaceDark,
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: kBorder),
                    ),
                    enabledBorder: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: kBorder),
                    ),
                    focusedBorder: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: kNeonCyan),
                    ),
                    contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
                  ),
                ),
                const SizedBox(height: 16),

                // Mediator WS
                const SectionLabel(text: 'MEDIATOR WEBSOCKET URL'),
                const SizedBox(height: 8),
                TextField(
                  controller: _mediatorController,
                  style: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextPrimary),
                  decoration: InputDecoration(
                    hintText: 'wss://mediator.example.com',
                    hintStyle: GoogleFonts.jetBrainsMono(fontSize: 14, color: kTextTertiary),
                    filled: true,
                    fillColor: kSurfaceDark,
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: kBorder),
                    ),
                    enabledBorder: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: kBorder),
                    ),
                    focusedBorder: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: const BorderSide(color: kNeonCyan),
                    ),
                    contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
                  ),
                ),
                const SizedBox(height: 20),

                // Generate identity
                GestureDetector(
                  onTap: _hubController.text.isEmpty ? null : _generateIdentity,
                  child: Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    decoration: glassDecoration(
                      accentBorder: _hubController.text.isEmpty ? kBorder : kNeonCyan.withValues(alpha: 0.3),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Icon(LucideIcons.key, size: 18, color: _hubController.text.isEmpty ? kTextTertiary : kNeonCyan),
                        const SizedBox(width: 8),
                        Text(
                          _generating ? '生成中...' : '生成商户身份',
                          style: GoogleFonts.inter(
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                            color: _hubController.text.isEmpty ? kTextTertiary : kNeonCyan,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 16),

                // DID display
                if (svc.did.isNotEmpty) ...[
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(14),
                    decoration: glassDecoration(),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('DID', style: sectionLabel()),
                        const SizedBox(height: 4),
                        Row(
                          children: [
                            Expanded(
                              child: Text(svc.did,
                                  style: monoValue(12),
                                  overflow: TextOverflow.ellipsis),
                            ),
                            GestureDetector(
                              onTap: () => Clipboard.setData(ClipboardData(text: svc.did)),
                              child: const Icon(LucideIcons.copy, size: 16, color: kTextSecondary),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                ],

                const SizedBox(height: 32),

                // Start button
                GestureDetector(
                  onTap: _identityReady ? _start : null,
                  child: Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(vertical: 16),
                    decoration: BoxDecoration(
                      gradient: _identityReady
                          ? const LinearGradient(colors: [kNeonCyan, kNeonCyanDim])
                          : null,
                      color: _identityReady ? null : kSurfaceElevated,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Text(
                      '开始使用',
                      textAlign: TextAlign.center,
                      style: GoogleFonts.inter(
                        fontSize: 15,
                        fontWeight: FontWeight.w700,
                        color: _identityReady ? kBackground : kTextTertiary,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
