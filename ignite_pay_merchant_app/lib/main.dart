import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
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

class _AppShellState extends State<_AppShell> {
  bool? _onboarded;

  @override
  void initState() {
    super.initState();
    _checkOnboarding();
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
    setState(() => _onboarded = hub.isNotEmpty);
  }

  void _onOnboardingComplete() {
    setState(() => _onboarded = true);
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
    return const _MainNavigator();
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
