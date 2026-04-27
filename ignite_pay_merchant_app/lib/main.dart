import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:local_auth/local_auth.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_merchant/theme.dart';
import 'package:ignite_pay_merchant/dashboard_screen.dart';
import 'package:ignite_pay_merchant/payment_list_screen.dart';
import 'package:ignite_pay_merchant/settings_screen.dart';
import 'package:ignite_pay_merchant/onboarding_screen.dart';
import 'package:ignite_pay_merchant/services/merchant_service.dart';
import 'package:ignite_pay_merchant/services/channel_service.dart';
import 'package:ignite_pay_merchant/services/voice_service.dart';
import 'package:ignite_pay_merchant/services/merchant_push_service.dart';
import 'package:provider/provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  final merchantService = MerchantService();
  await merchantService.initialize();

  final channelService = ChannelService();
  await channelService.initialize();

  final voiceService = VoiceService();
  await voiceService.initialize();

  final pushService = MerchantPushService();

  runApp(MultiProvider(
    providers: [
      ChangeNotifierProvider.value(value: merchantService),
      ChangeNotifierProvider.value(value: channelService),
      ChangeNotifierProvider.value(value: voiceService),
      ChangeNotifierProvider.value(value: pushService),
    ],
    child: const MerchantApp(),
  ));
}

// ---------------------------------------------------------------------------
// App Root
// ---------------------------------------------------------------------------
class MerchantApp extends StatelessWidget {
  const MerchantApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Ignite Merchant',
      theme: ThemeData(
        brightness: Brightness.dark,
        scaffoldBackgroundColor: kBackground,
        colorScheme: const ColorScheme.dark(
          primary: kNeonCyan,
          surface: kSurfaceDark,
        ),
        textTheme: GoogleFonts.interTextTheme(
          ThemeData.dark().textTheme,
        ),
      ),
      home: const _AppShell(),
    );
  }
}

// ---------------------------------------------------------------------------
// App Shell: Onboarding or Main Navigator
// ---------------------------------------------------------------------------
class _AppShell extends StatefulWidget {
  const _AppShell();

  @override
  State<_AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<_AppShell> with WidgetsBindingObserver {
  bool? _onboarded;
  bool _isLocked = false;
  bool _authenticating = false;
  final _localAuth = LocalAuthentication();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _checkOnboarding();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused && _onboarded == true) {
      setState(() { _isLocked = true; });
    } else if (state == AppLifecycleState.resumed && _isLocked) {
      _authenticate();
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // Connect payment confirmation callback to voice service
    final merchantService = context.read<MerchantService>();
    final voiceService = context.read<VoiceService>();
    merchantService.setOnPaymentConfirmed((order) {
      voiceService.announcePayment(order.amount);
    });
  }

  Future<void> _checkOnboarding() async {
    final prefs = await SharedPreferences.getInstance();
    final hub = prefs.getString('hub_endpoint') ?? '';
    final onboarded = hub.isNotEmpty;
    if (mounted) {
      setState(() {
        _onboarded = onboarded;
        _isLocked = onboarded;
      });
      if (onboarded) _authenticate();
    }
  }

  Future<void> _authenticate() async {
    if (_authenticating) return;
    _authenticating = true;
    try {
      final canAuth = await _localAuth.canCheckBiometrics || await _localAuth.isDeviceSupported();
      if (!canAuth) {
        if (mounted) setState(() { _isLocked = false; });
        return;
      }
      final authenticated = await _localAuth.authenticate(
        localizedReason: 'Unlock Ignite Merchant',
        biometricOnly: false,
        persistAcrossBackgrounding: true,
      );
      if (authenticated && mounted) {
        setState(() { _isLocked = false; });
      }
    } catch (e) {
      debugPrint('Auth error: $e');
      if (mounted) setState(() { _isLocked = false; });
    } finally {
      _authenticating = false;
    }
  }

  void _onOnboardingComplete() {
    setState(() { _onboarded = true; _isLocked = true; });
    _authenticate();
  }

  @override
  Widget build(BuildContext context) {
    if (_onboarded == null) {
      return const Scaffold(
        backgroundColor: kBackground,
        body: Center(child: CircularProgressIndicator(color: kNeonCyan)),
      );
    }
    if (!_onboarded!) {
      return OnboardingScreen(onComplete: _onOnboardingComplete);
    }
    if (_isLocked) {
      return _MerchantLockScreen(onUnlock: _authenticate);
    }
    return const _MainNavigator();
  }
}

// ---------------------------------------------------------------------------
// Lock Screen (biometric / PIN)
// ---------------------------------------------------------------------------
class _MerchantLockScreen extends StatelessWidget {
  final VoidCallback onUnlock;
  const _MerchantLockScreen({required this.onUnlock});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
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
              ),
              child: const Icon(LucideIcons.store, size: 36, color: kBackground),
            ),
            const SizedBox(height: 24),
            Text(
              'Ignite Merchant is locked',
              style: GoogleFonts.inter(
                fontSize: 20,
                fontWeight: FontWeight.w600,
                color: kTextPrimary,
              ),
            ),
            const SizedBox(height: 8),
            Text(
              'Authenticate to continue',
              style: GoogleFonts.inter(
                fontSize: 14,
                color: kTextSecondary,
              ),
            ),
            const SizedBox(height: 32),
            GestureDetector(
              onTap: onUnlock,
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 14),
                decoration: BoxDecoration(
                  gradient: const LinearGradient(colors: [kNeonCyan, kNeonCyanDim]),
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(LucideIcons.lock, size: 18, color: kBackground),
                    const SizedBox(width: 8),
                    Text(
                      'Unlock',
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
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Main Navigator with Bottom Nav
// ---------------------------------------------------------------------------
class _MainNavigator extends StatefulWidget {
  const _MainNavigator();

  @override
  State<_MainNavigator> createState() => _MainNavigatorState();
}

class _MainNavigatorState extends State<_MainNavigator> {
  int _currentIndex = 0;

  final _pages = const [
    DashboardScreen(),
    PaymentListScreen(),
    SettingsScreen(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: IndexedStack(
        index: _currentIndex,
        children: _pages,
      ),
      bottomNavigationBar: Container(
        decoration: BoxDecoration(
          color: kSurfaceDark.withValues(alpha: 0.95),
          border: Border(top: BorderSide(color: kGlassBorder)),
        ),
        child: SafeArea(
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.spaceAround,
              children: [
                _NavItem(
                  icon: LucideIcons.home,
                  label: '首页',
                  selected: _currentIndex == 0,
                  onTap: () => setState(() => _currentIndex = 0),
                ),
                _NavItem(
                  icon: LucideIcons.receipt,
                  label: '收款',
                  selected: _currentIndex == 1,
                  onTap: () => setState(() => _currentIndex = 1),
                ),
                _NavItem(
                  icon: LucideIcons.settings,
                  label: '设置',
                  selected: _currentIndex == 2,
                  onTap: () => setState(() => _currentIndex = 2),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _NavItem extends StatelessWidget {
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  const _NavItem({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final color = selected ? kNeonCyan : kTextSecondary;
    return GestureDetector(
      onTap: onTap,
      behavior: HitTestBehavior.opaque,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 22, color: color),
            const SizedBox(height: 3),
            Text(label,
                style: GoogleFonts.inter(
                  fontSize: 10,
                  fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                  color: color,
                )),
          ],
        ),
      ),
    );
  }
}
