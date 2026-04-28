import 'dart:math';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:local_auth/local_auth.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/src/rust/frb_generated.dart';
import 'package:ignite_pay_app/challenge_screen.dart';
import 'package:ignite_pay_app/policy_screen.dart';
import 'package:ignite_pay_app/vault_screen.dart';
import 'package:ignite_pay_app/messages_screen.dart';
import 'package:ignite_pay_app/settings_screen.dart';
import 'package:ignite_pay_app/notification_screen.dart';
import 'package:ignite_pay_app/channel_topology_screen.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
import 'package:ignite_pay_app/onboarding_screen.dart';
import 'package:provider/provider.dart';
import 'package:app_links/app_links.dart';
import 'package:shared_preferences/shared_preferences.dart';

// ---------------------------------------------------------------------------
// Theme Constants
// ---------------------------------------------------------------------------
const _kBackground = Color(0xFF0F0F1A);
const _kSurfaceDark = Color(0xFF1A1A2E);
const _kSurfaceMid = Color(0xFF16213E);
const _kNeonCyan = Color(0xFFFF5722);
const _kNeonCyanDim = Color(0xFFBF360C);
const _kTextPrimary = Color(0xFFE8E8F0);
const _kTextSecondary = Color(0xFF8A8AA0);
const _kSuccess = Color(0xFF00E676);
const _kPending = Color(0xFFFFB300);
const _kAmber = Color(0xFFFFB300);
const _kIntercepted = Color(0xFFFF5252);
const _kGlassBorder = Color(0x1AFFFFFF);

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------
Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();

  runApp(const IgnitePayApp());
}

// ---------------------------------------------------------------------------
// App Root
// ---------------------------------------------------------------------------
class IgnitePayApp extends StatelessWidget {
  const IgnitePayApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ChangeNotifierProvider(
      create: (_) => DidcommService(),
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        title: 'Ignite Pay',
        theme: ThemeData(
          brightness: Brightness.dark,
          scaffoldBackgroundColor: _kBackground,
          colorScheme: const ColorScheme.dark(
            primary: _kNeonCyan,
            surface: _kSurfaceDark,
          ),
          textTheme: GoogleFonts.interTextTheme(
            ThemeData.dark().textTheme,
          ),
        ),
        home: const _AppShell(),
      ),
    );
  }
}

// Shell that decides: onboarding or main navigator
class _AppShell extends StatefulWidget {
  const _AppShell();

  @override
  State<_AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<_AppShell> with WidgetsBindingObserver {
  bool _loading = true;
  bool _showOnboarding = false;
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
    if (state == AppLifecycleState.paused && !_loading && !_showOnboarding) {
      setState(() { _isLocked = true; });
    } else if (state == AppLifecycleState.resumed && _isLocked) {
      _authenticate();
    }
  }

  String? _autoConnectError;

  Future<void> _checkOnboarding() async {
    final prefs = await SharedPreferences.getInstance();
    final hasDid = prefs.getBool('onboarding_complete') ?? false;

    if (hasDid) {
      // Load existing identity
      final svc = context.read<DidcommService>();
      await svc.initialize();
      // Auto-reconnect mediator if URL was previously saved
      final wsUrl = svc.mediatorWsUrl;
      if (wsUrl.isNotEmpty) {
        try {
          await svc.connectToMediator(wsUrl);
        } catch (e) {
          _autoConnectError = e.toString();
        }
      }
      if (mounted) {
        setState(() {
          _loading = false;
          _isLocked = true;
        });
        _authenticate();
      }
    } else {
      if (mounted) setState(() { _loading = false; _showOnboarding = true; });
    }
  }

  Future<void> _authenticate() async {
    if (_authenticating) return;
    _authenticating = true;
    try {
      final canAuth = await _localAuth.canCheckBiometrics || await _localAuth.isDeviceSupported();
      if (!canAuth) {
        if (mounted) setState(() { _isLocked = false; });
        _showAutoConnectErrorIfAny();
        return;
      }
      final authenticated = await _localAuth.authenticate(
        localizedReason: 'Unlock Ignite Pay',
        biometricOnly: false,
        persistAcrossBackgrounding: true,
      );
      if (authenticated && mounted) {
        setState(() { _isLocked = false; });
        _showAutoConnectErrorIfAny();
      }
    } catch (e) {
      debugPrint('Auth error: $e');
      // Fallback: unlock anyway (e.g. emulator with no biometric hardware)
      if (mounted) setState(() { _isLocked = false; });
      _showAutoConnectErrorIfAny();
    } finally {
      _authenticating = false;
    }
  }

  void _showAutoConnectErrorIfAny() {
    final err = _autoConnectError;
    if (err == null) return;
    _autoConnectError = null;
    Future.microtask(() {
      if (!mounted) return;
      showDialog(
        context: context,
        builder: (ctx) => AlertDialog(
          backgroundColor: const Color(0xFF1A1A2E),
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
          title: Row(
            children: [
              const Icon(LucideIcons.wifiOff, size: 20, color: Color(0xFFFF5252)),
              const SizedBox(width: 10),
              Text('Auto-Reconnect Failed',
                  style: GoogleFonts.inter(
                      fontSize: 16, fontWeight: FontWeight.w600, color: Color(0xFFE8E8F0))),
            ],
          ),
          content: Text('Could not connect to mediator:\n$err',
              style: GoogleFonts.inter(fontSize: 13, color: Color(0xFF8A8AA0))),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(),
              child: Text('OK',
                  style: GoogleFonts.inter(fontWeight: FontWeight.w600, color: const Color(0xFFFF5722))),
            ),
          ],
        ),
      );
    });
  }

  Future<void> _onOnboardingComplete() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('onboarding_complete', true);
    setState(() { _showOnboarding = false; _isLocked = true; });
    _authenticate();
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return Scaffold(
        backgroundColor: _kBackground,
        body: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 64,
                height: 64,
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(16),
                  gradient: const LinearGradient(
                    colors: [_kNeonCyan, _kNeonCyanDim],
                    begin: Alignment.topLeft,
                    end: Alignment.bottomRight,
                  ),
                ),
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(16),
                  child: Image.asset('assets/icons/ignite_pay.png', width: 64, height: 64, fit: BoxFit.cover),
                ),
              ),
              const SizedBox(height: 20),
              CircularProgressIndicator(color: _kNeonCyan.withValues(alpha: 0.7), strokeWidth: 2),
            ],
          ),
        ),
      );
    }
    if (_showOnboarding) {
      return OnboardingScreen(onComplete: _onOnboardingComplete);
    }
    if (_isLocked) {
      return _LockScreen(onUnlock: _authenticate);
    }
    return const _MainNavigator();
  }
}

// ---------------------------------------------------------------------------
// Lock Screen (biometric / PIN)
// ---------------------------------------------------------------------------
class _LockScreen extends StatelessWidget {
  final VoidCallback onUnlock;
  const _LockScreen({required this.onUnlock});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: _kBackground,
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
                  colors: [_kNeonCyan, _kNeonCyanDim],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
              ),
              child: ClipRRect(
                borderRadius: BorderRadius.circular(20),
                child: Image.asset('assets/icons/ignite_pay.png', width: 80, height: 80, fit: BoxFit.cover),
              ),
            ),
            const SizedBox(height: 24),
            Text(
              'Ignite Pay is locked',
              style: GoogleFonts.inter(
                fontSize: 20,
                fontWeight: FontWeight.w600,
                color: _kTextPrimary,
              ),
            ),
            const SizedBox(height: 8),
            Text(
              'Authenticate to continue',
              style: GoogleFonts.inter(
                fontSize: 14,
                color: _kTextSecondary,
              ),
            ),
            const SizedBox(height: 32),
            GestureDetector(
              onTap: onUnlock,
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 14),
                decoration: BoxDecoration(
                  gradient: const LinearGradient(colors: [_kNeonCyan, _kNeonCyanDim]),
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(LucideIcons.lock, size: 18, color: _kBackground),
                    const SizedBox(width: 8),
                    Text(
                      'Unlock',
                      style: GoogleFonts.inter(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: _kBackground,
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
// Main Navigator with Bottom Nav Bar
// ---------------------------------------------------------------------------
class _MainNavigator extends StatefulWidget {
  const _MainNavigator();

  @override
  State<_MainNavigator> createState() => _MainNavigatorState();
}

class _MainNavigatorState extends State<_MainNavigator> {
  int _currentIndex = 0;
  Stream<Uri>? _deepLinkStream;

  final _pages = const [
    IgnitePayDashboard(),
    _MessagesTabPage(),
    _SettingsTabPage(),
  ];

  @override
  void initState() {
    super.initState();
    _initDeepLinks();
  }

  void _initDeepLinks() {
    try {
      final appLinks = AppLinks();
      _deepLinkStream = appLinks.uriLinkStream;
      _deepLinkStream!.listen(_handleDeepLink);
    } catch (e) {
      debugPrint('Deep links init failed (non-fatal): $e');
    }
  }

  void _handleDeepLink(Uri uri) {
    if (uri.scheme == 'ignitepay' && uri.host == 'onchain') {
      final signature = uri.queryParameters['signature'];
      if (signature != null) {
        debugPrint('Deep link callback: signature=$signature');
        final svc = SessionKeyService();
        svc.completeRegistration(signature).then((session_info) {
          debugPrint('Session key registered: ${session_info.ephemeralPubkey}');
        }).catchError((e) {
          debugPrint('Deep link registration failed: $e');
        });
      }
    }
  }

  @override
  void dispose() {
    _deepLinkStream = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: IndexedStack(
        index: _currentIndex,
        children: _pages,
      ),
      bottomNavigationBar: Container(
        decoration: BoxDecoration(
          color: _kSurfaceDark.withValues(alpha: 0.95),
          border: Border(top: BorderSide(color: _kGlassBorder)),
        ),
        child: SafeArea(
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.spaceAround,
              children: [
                _NavItem(
                  icon: LucideIcons.home,
                  label: 'Home',
                  selected: _currentIndex == 0,
                  onTap: () => setState(() => _currentIndex = 0),
                ),
                _NavItem(
                  icon: LucideIcons.mail,
                  label: 'Messages',
                  selected: _currentIndex == 1,
                  onTap: () => setState(() => _currentIndex = 1),
                ),
                _NavItem(
                  icon: LucideIcons.settings,
                  label: 'Settings',
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
    final color = selected ? _kNeonCyan : _kTextSecondary;
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
            Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 10,
                fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                color: color,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// Thin wrappers to embed pages in the tab
class _MessagesTabPage extends StatelessWidget {
  const _MessagesTabPage();
  @override
  Widget build(BuildContext context) {
    return const _EmbeddedMessagesScreen();
  }
}

class _SettingsTabPage extends StatelessWidget {
  const _SettingsTabPage();
  @override
  Widget build(BuildContext context) {
    return const _EmbeddedSettingsScreen();
  }
}

// These are lightweight versions that skip the push navigation
// since they're already embedded in tabs
class _EmbeddedMessagesScreen extends StatelessWidget {
  const _EmbeddedMessagesScreen();
  @override
  Widget build(BuildContext context) {
    // Delegate to the full messages screen widget
    return const MessagesScreen();
  }
}

class _EmbeddedSettingsScreen extends StatelessWidget {
  const _EmbeddedSettingsScreen();
  @override
  Widget build(BuildContext context) {
    return const SettingsScreen();
  }
}

// ---------------------------------------------------------------------------
// Dashboard Screen
// ---------------------------------------------------------------------------
class IgnitePayDashboard extends StatelessWidget {
  const IgnitePayDashboard({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          child: Column(
            children: [
              const _DashboardHeader(),
              const SizedBox(height: 20),
              Expanded(
                child: SingleChildScrollView(
                  child: Column(
                    children: [
                      const DIDCard(),
                      const SizedBox(height: 20),
                      const _QuickNavRow(),
                      const SizedBox(height: 20),
                      const TrustScoreGauge(
                        spent: 0.42,
                        limit: 1.0,
                        spentLabel: '0.42 SOL',
                        limitLabel: '1.00 SOL',
                      ),
                      const SizedBox(height: 24),
                      Consumer<DidcommService>(
                        builder: (context, svc, _) {
                          return ActivityFeed(messages: svc.messages);
                        },
                      ),
                      const SizedBox(height: 24),
                      const _AuthAction(),
                    ],
                  ),
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
// Header
// ---------------------------------------------------------------------------
class _DashboardHeader extends StatelessWidget {
  const _DashboardHeader();

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(10),
                gradient: const LinearGradient(
                  colors: [_kNeonCyan, _kNeonCyanDim],
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
              ),
              child: ClipRRect(
                borderRadius: BorderRadius.circular(10),
                child: Image.asset('assets/icons/ignite_pay.png', width: 36, height: 36, fit: BoxFit.cover),
              ),
            ),
            const SizedBox(width: 12),
            Text(
              'Ignite Pay',
              style: GoogleFonts.inter(
                fontSize: 22,
                fontWeight: FontWeight.w700,
                color: _kTextPrimary,
                letterSpacing: -0.5,
              ),
            ),
          ],
        ),
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Consumer<DidcommService>(
              builder: (context, svc, _) {
                final unreadCount = svc.messages
                    .where((m) => !m.msgType.contains('payment-auth-request'))
                    .length;
                return GestureDetector(
                  onTap: () => openNotificationCenter(context),
                  child: Container(
                    width: 36,
                    height: 36,
                    decoration: BoxDecoration(
                      color: _kSurfaceMid.withValues(alpha: 0.6),
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: _kGlassBorder),
                    ),
                    child: Stack(
                      clipBehavior: Clip.none,
                      children: [
                        const Center(
                          child: Icon(LucideIcons.bell, size: 18, color: _kTextSecondary),
                        ),
                        if (unreadCount > 0)
                          Positioned(
                            right: 4,
                            top: 4,
                            child: Container(
                              width: 8,
                              height: 8,
                              decoration: const BoxDecoration(
                                color: _kNeonCyan,
                                shape: BoxShape.circle,
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                );
              },
            ),
            const SizedBox(width: 10),
            GestureDetector(
              onTap: () => openPolicyArchitect(context),
              child: Container(
                width: 36,
                height: 36,
                decoration: BoxDecoration(
                  color: _kSurfaceMid.withValues(alpha: 0.6),
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(color: _kGlassBorder),
                ),
                child: const Icon(LucideIcons.settings2, size: 18, color: _kTextSecondary),
              ),
            ),
            const SizedBox(width: 10),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(
                color: _kSuccess.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(20),
                border: Border.all(color: _kSuccess.withValues(alpha: 0.3)),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 7,
                    height: 7,
                    decoration: const BoxDecoration(
                      color: _kSuccess,
                      shape: BoxShape.circle,
                    ),
                  ),
                  const SizedBox(width: 6),
                  Text(
                    'Mainnet',
                    style: GoogleFonts.inter(
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      color: _kSuccess,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Quick Nav Row (Vault & Policy shortcuts)
// ---------------------------------------------------------------------------
class _QuickNavRow extends StatelessWidget {
  const _QuickNavRow();

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: _QuickNavCard(
            icon: LucideIcons.lock,
            label: 'Vault',
            subtitle: 'Keys & Identity',
            gradientColors: [const Color(0xFFFF8A50), const Color(0xFFE64A19)],
            onTap: () => openVaultIdentity(context),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: _QuickNavCard(
            icon: LucideIcons.shield,
            label: 'Policies',
            subtitle: 'Spending rules',
            gradientColors: [_kNeonCyan, _kNeonCyanDim],
            onTap: () => openPolicyArchitect(context),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          child: _QuickNavCard(
            icon: LucideIcons.layers,
            label: 'Channels',
            subtitle: 'State channels',
            gradientColors: [const Color(0xFFFF6E40), const Color(0xFFE65100)],
            onTap: () => openChannelTopology(context),
          ),
        ),
      ],
    );
  }
}

class _QuickNavCard extends StatelessWidget {
  final IconData icon;
  final String label;
  final String subtitle;
  final List<Color> gradientColors;
  final VoidCallback onTap;

  const _QuickNavCard({
    required this.icon,
    required this.label,
    required this.subtitle,
    required this.gradientColors,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 14, horizontal: 10),
        decoration: BoxDecoration(
          color: _kSurfaceDark.withValues(alpha: 0.6),
          borderRadius: BorderRadius.circular(14),
          border: Border.all(color: _kGlassBorder),
        ),
        child: Column(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(10),
                gradient: LinearGradient(
                  colors: gradientColors,
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                ),
              ),
              child: Icon(icon, size: 18, color: _kBackground),
            ),
            const SizedBox(height: 10),
            Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 13,
                fontWeight: FontWeight.w600,
                color: _kTextPrimary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 2),
            Text(
              subtitle,
              style: GoogleFonts.inter(
                fontSize: 10,
                color: _kTextSecondary,
              ),
              textAlign: TextAlign.center,
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// DID Identity Card (Glassmorphism)
// ---------------------------------------------------------------------------
class DIDCard extends StatefulWidget {
  const DIDCard({super.key});

  @override
  State<DIDCard> createState() => _DIDCardState();
}

class _DIDCardState extends State<DIDCard> {
  bool _copied = false;

  String get _did => DidcommService().did;

  void _copyToClipboard() {
    Clipboard.setData(ClipboardData(text: _did));
    setState(() => _copied = true);
    Future.delayed(const Duration(seconds: 2), () {
      if (mounted) setState(() => _copied = false);
    });
  }

  @override
  Widget build(BuildContext context) {
    final didService = DidcommService();

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: _kSurfaceMid.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: _kGlassBorder),
        gradient: LinearGradient(
          colors: [
            _kSurfaceMid.withValues(alpha: 0.7),
            _kSurfaceDark.withValues(alpha: 0.5),
          ],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(LucideIcons.fingerprint, size: 16, color: _kNeonCyan.withValues(alpha: 0.8)),
              const SizedBox(width: 8),
              Text(
                'IDENTITY',
                style: GoogleFonts.inter(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: _kTextSecondary,
                  letterSpacing: 1.2,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: Text(
                  _did,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 14,
                    fontWeight: FontWeight.w500,
                    color: _kTextPrimary,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(width: 8),
              _GlassIconButton(
                icon: _copied ? LucideIcons.check : LucideIcons.copy,
                onTap: _copyToClipboard,
                isActive: _copied,
              ),
            ],
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              _ConnectionDot(connected: didService.isConnected),
              const SizedBox(width: 8),
              Text(
                didService.isConnected ? 'Connection Live' : 'Disconnected',
                style: GoogleFonts.inter(
                  fontSize: 12,
                  fontWeight: FontWeight.w500,
                  color: (didService.isConnected ? _kSuccess : _kIntercepted).withValues(alpha: 0.9),
                ),
              ),
              const Spacer(),
              if (didService.pendingMessageCount > 0)
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                  decoration: BoxDecoration(
                    color: _kPending.withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(color: _kPending.withValues(alpha: 0.3)),
                  ),
                  child: Text(
                    '${didService.pendingMessageCount} msg${didService.pendingMessageCount != 1 ? 's' : ''}',
                    style: GoogleFonts.inter(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: _kPending,
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

// ---------------------------------------------------------------------------
// Connection Status Dot (pulsating when connected, static when not)
// ---------------------------------------------------------------------------
class _ConnectionDot extends StatefulWidget {
  final bool connected;
  const _ConnectionDot({required this.connected});

  @override
  State<_ConnectionDot> createState() => _ConnectionDotState();
}

class _ConnectionDotState extends State<_ConnectionDot>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ctrl;

  @override
  void initState() {
    super.initState();
    _ctrl = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1500),
    )..repeat(reverse: true);
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final color = widget.connected ? _kSuccess : _kIntercepted;

    if (!widget.connected) {
      return Container(
        width: 9,
        height: 9,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: color,
        ),
      );
    }

    return AnimatedBuilder(
      animation: _ctrl,
      builder: (context, child) {
        return Container(
          width: 9,
          height: 9,
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: color,
            boxShadow: [
              BoxShadow(
                color: color.withValues(alpha: 0.4 + 0.4 * _ctrl.value),
                blurRadius: 6 + 4 * _ctrl.value,
                spreadRadius: 1,
              ),
            ],
          ),
        );
      },
    );
  }
}

// ---------------------------------------------------------------------------
// Trust Score / Daily Allowance Gauge
// ---------------------------------------------------------------------------
class TrustScoreGauge extends StatelessWidget {
  final double spent;
  final double limit;
  final String spentLabel;
  final String limitLabel;

  const TrustScoreGauge({
    super.key,
    required this.spent,
    required this.limit,
    required this.spentLabel,
    required this.limitLabel,
  });

  @override
  Widget build(BuildContext context) {
    final remaining = (limit - spent).clamp(0.0, limit);
    final pct = spent / limit;

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(24),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: _kGlassBorder),
      ),
      child: Column(
        children: [
          Row(
            children: [
              Icon(LucideIcons.gauge, size: 16, color: _kNeonCyan.withValues(alpha: 0.8)),
              const SizedBox(width: 8),
              Text(
                'DAILY ALLOWANCE',
                style: GoogleFonts.inter(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: _kTextSecondary,
                  letterSpacing: 1.2,
                ),
              ),
            ],
          ),
          const SizedBox(height: 20),
          SizedBox(
            width: 180,
            height: 180,
            child: CustomPaint(
              painter: _RadialGaugePainter(progress: pct),
              child: Center(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      '${(remaining * 100).toStringAsFixed(0)}%',
                      style: GoogleFonts.inter(
                        fontSize: 28,
                        fontWeight: FontWeight.w700,
                        color: _kTextPrimary,
                      ),
                    ),
                    Text(
                      'Remaining',
                      style: GoogleFonts.inter(
                        fontSize: 12,
                        color: _kTextSecondary,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
          const SizedBox(height: 16),
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              _GaugeLabel(value: spentLabel, label: 'Spent', color: _kNeonCyan),
              const SizedBox(width: 32),
              _GaugeLabel(value: limitLabel, label: 'Limit', color: _kTextSecondary),
            ],
          ),
        ],
      ),
    );
  }
}

class _GaugeLabel extends StatelessWidget {
  final String value;
  final String label;
  final Color color;

  const _GaugeLabel({
    required this.value,
    required this.label,
    required this.color,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Text(
          value,
          style: GoogleFonts.jetBrainsMono(
            fontSize: 16,
            fontWeight: FontWeight.w600,
            color: color,
          ),
        ),
        const SizedBox(height: 2),
        Text(
          label,
          style: GoogleFonts.inter(
            fontSize: 11,
            color: _kTextSecondary,
          ),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Radial Gauge Painter
// ---------------------------------------------------------------------------
class _RadialGaugePainter extends CustomPainter {
  final double progress;

  _RadialGaugePainter({required this.progress});

  @override
  void paint(Canvas canvas, Size size) {
    final center = Offset(size.width / 2, size.height / 2);
    final radius = (size.shortestSide / 2) - 14;
    const strokeWidth = 10.0;

    // Background track
    final bgPaint = Paint()
      ..color = _kSurfaceMid
      ..style = PaintingStyle.stroke
      ..strokeWidth = strokeWidth
      ..strokeCap = StrokeCap.round;
    canvas.drawCircle(center, radius, bgPaint);

    // Progress arc
    if (progress > 0) {
      final sweepAngle = 2 * pi * progress;
      final rect = Rect.fromCircle(center: center, radius: radius);
      final progressPaint = Paint()
        ..shader = SweepGradient(
          startAngle: -pi / 2,
          endAngle: -pi / 2 + sweepAngle,
          colors: const [_kNeonCyan, _kNeonCyanDim],
          stops: const [0.0, 1.0],
        ).createShader(rect)
        ..style = PaintingStyle.stroke
        ..strokeWidth = strokeWidth
        ..strokeCap = StrokeCap.round;
      canvas.drawArc(rect, -pi / 2, sweepAngle, false, progressPaint);
    }

    // Glow at the end of the progress arc
    if (progress > 0 && progress < 1) {
      final endAngle = -pi / 2 + 2 * pi * progress;
      final glowOffset = Offset(
        center.dx + radius * cos(endAngle),
        center.dy + radius * sin(endAngle),
      );
      final glowPaint = Paint()
        ..color = _kNeonCyan.withValues(alpha: 0.5)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 8);
      canvas.drawCircle(glowOffset, 6, glowPaint);
    }
  }

  @override
  bool shouldRepaint(covariant _RadialGaugePainter old) => old.progress != progress;
}

// ---------------------------------------------------------------------------
// Activity Feed
// ---------------------------------------------------------------------------
class ActivityFeed extends StatelessWidget {
  final List<DecryptedMsg> messages;
  const ActivityFeed({super.key, required this.messages});

  @override
  Widget build(BuildContext context) {
    // Filter to payment-related messages
    final paymentMsgs = messages
        .where((m) => m.msgType.contains('payment'))
        .toList()
        .reversed
        .take(5)
        .toList();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(LucideIcons.activity, size: 16, color: _kNeonCyan.withValues(alpha: 0.8)),
            const SizedBox(width: 8),
            Text(
              'RECENT ACTIVITY',
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: _kTextSecondary,
                letterSpacing: 1.2,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        if (paymentMsgs.isEmpty)
          Center(
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 20),
              child: Column(
                children: [
                  Icon(LucideIcons.inbox, size: 32, color: _kTextSecondary.withValues(alpha: 0.4)),
                  const SizedBox(height: 8),
                  Text(
                    'No recent activity',
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      color: _kTextSecondary,
                    ),
                  ),
                ],
              ),
            ),
          )
        else
          ...paymentMsgs.map((msg) {
            final status = msg.msgType.contains('auth-request')
                ? _ActivityStatus.pending
                : _ActivityStatus.success;
            final icon = status == _ActivityStatus.pending
                ? LucideIcons.creditCard
                : LucideIcons.checkCircle2;
            final amount = msg.amount != null
                ? '${(msg.amount! / 1e9).toStringAsFixed(4)} SOL'
                : '--';
            final merchant = msg.merchantDid != null && msg.merchantDid!.isNotEmpty
                ? (msg.merchantDid!.length > 24
                    ? '${msg.merchantDid!.substring(0, 16)}...${msg.merchantDid!.substring(msg.merchantDid!.length - 6)}'
                    : msg.merchantDid!)
                : 'Unknown';
            return _ActivityTile(
              item: _ActivityItem(
                merchant: merchant,
                amount: amount,
                time: msg.msgType.contains('auth-request') ? 'Pending' : 'Processed',
                status: status,
                icon: icon,
              ),
            );
          }),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Activity Model
// ---------------------------------------------------------------------------
enum _ActivityStatus { success, pending, intercepted }

class _ActivityItem {
  final String merchant;
  final String amount;
  final String time;
  final _ActivityStatus status;
  final IconData icon;

  const _ActivityItem({
    required this.merchant,
    required this.amount,
    required this.time,
    required this.status,
    required this.icon,
  });
}

// ---------------------------------------------------------------------------
// Activity Tile
// ---------------------------------------------------------------------------
class _ActivityTile extends StatelessWidget {
  final _ActivityItem item;

  const _ActivityTile({required this.item});

  Color get _statusColor => switch (item.status) {
        _ActivityStatus.success => _kSuccess,
        _ActivityStatus.pending => _kPending,
        _ActivityStatus.intercepted => _kIntercepted,
      };

  String get _statusLabel => switch (item.status) {
        _ActivityStatus.success => 'Success',
        _ActivityStatus.pending => 'Pending',
        _ActivityStatus.intercepted => 'Intercepted',
      };

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
        decoration: BoxDecoration(
          color: _kSurfaceDark.withValues(alpha: 0.5),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: _kGlassBorder),
        ),
        child: Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                color: _statusColor.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(item.icon, size: 18, color: _statusColor),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    item.merchant,
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                      color: _kTextPrimary,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    item.time,
                    style: GoogleFonts.inter(
                      fontSize: 11,
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
                  item.amount,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: _kTextPrimary,
                  ),
                ),
                const SizedBox(height: 4),
                _StatusBadge(color: _statusColor, label: _statusLabel),
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
// Glass Icon Button (shadcn-inspired)
// ---------------------------------------------------------------------------
class _GlassIconButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback onTap;
  final bool isActive;

  const _GlassIconButton({
    required this.icon,
    required this.onTap,
    this.isActive = false,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: 34,
        height: 34,
        decoration: BoxDecoration(
          color: isActive
              ? _kSuccess.withValues(alpha: 0.15)
              : _kSurfaceMid.withValues(alpha: 0.6),
          borderRadius: BorderRadius.circular(8),
          border: Border.all(
            color: isActive
                ? _kSuccess.withValues(alpha: 0.3)
                : _kGlassBorder,
          ),
        ),
        child: Icon(
          icon,
          size: 16,
          color: isActive ? _kSuccess : _kTextSecondary,
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Auth Action (launches X402 Challenge)
// ---------------------------------------------------------------------------
class _AuthAction extends StatefulWidget {
  const _AuthAction();

  @override
  State<_AuthAction> createState() => _AuthActionState();
}

class _AuthActionState extends State<_AuthAction> {
  Stream<AuthRequest>? _authStream;

  @override
  void initState() {
    super.initState();
    // Listen for auth requests from DidcommService
    final didService = DidcommService();
    _authStream = didService.authRequests;
  }

  Future<void> _openChallenge(BuildContext context, {AuthRequest? request}) async {
    final result = await showX402Challenge<String>(context, request: request);
    if (result != null && context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          backgroundColor: result == 'authorized' ? _kSuccess : _kIntercepted,
          behavior: SnackBarBehavior.floating,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
          margin: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
          content: Text(
            result == 'authorized' ? 'Payment authorized' : 'Payment declined',
            style: GoogleFonts.inter(fontWeight: FontWeight.w600),
          ),
          duration: const Duration(seconds: 2),
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return StreamBuilder<AuthRequest>(
      stream: _authStream,
      builder: (context, snapshot) {
        // If we have a pending auth request, show a notification banner
        final hasPending = snapshot.hasData || DidcommService().pendingAuth != null;

        return Column(
          children: [
            if (hasPending)
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: GestureDetector(
                  onTap: () {
                    final request = DidcommService().pendingAuth ?? snapshot.data;
                    _openChallenge(context, request: request);
                  },
                  child: Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(vertical: 14, horizontal: 16),
                    decoration: BoxDecoration(
                      color: _kAmber.withValues(alpha: 0.12),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(color: _kAmber.withValues(alpha: 0.3)),
                    ),
                    child: Row(
                      children: [
                        const Icon(LucideIcons.bell, size: 18, color: _kAmber),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Text(
                            'Payment authorization requested',
                            style: GoogleFonts.inter(
                              fontSize: 13,
                              fontWeight: FontWeight.w600,
                              color: _kAmber,
                            ),
                          ),
                        ),
                        const Icon(LucideIcons.chevronRight, size: 16, color: _kAmber),
                      ],
                    ),
                  ),
                ),
              ),
            GestureDetector(
              onTap: () => _openChallenge(context),
              child: Container(
                width: double.infinity,
                padding: const EdgeInsets.symmetric(vertical: 14),
                decoration: BoxDecoration(
                  gradient: const LinearGradient(
                    colors: [_kNeonCyan, _kNeonCyanDim],
                  ),
                  borderRadius: BorderRadius.circular(12),
                  boxShadow: [
                    BoxShadow(
                      color: _kNeonCyan.withValues(alpha: 0.25),
                      blurRadius: 16,
                      spreadRadius: 2,
                    ),
                  ],
                ),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    const Icon(LucideIcons.zap, size: 18, color: _kBackground),
                    const SizedBox(width: 8),
                    Text(
                      'Authorize Payment',
                      style: GoogleFonts.inter(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: _kBackground,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ],
        );
      },
    );
  }
}
