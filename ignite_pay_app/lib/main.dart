// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:local_auth/local_auth.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/src/rust/frb_generated.dart';
import 'package:ignite_pay_app/src/rust/api/session.dart' as session;
import 'package:ignite_pay_app/challenge_screen.dart';
import 'package:ignite_pay_app/policy_screen.dart';
import 'package:ignite_pay_app/vault_screen.dart';
import 'package:ignite_pay_app/messages_screen.dart';
import 'package:ignite_pay_app/settings_screen.dart';
import 'package:ignite_pay_app/notification_screen.dart';
import 'package:ignite_pay_app/channel_topology_screen.dart';
import 'package:ignite_pay_app/qr_scanner_screen.dart';
import 'package:ignite_pay_app/qr_payment_screen.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/channel_service.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
import 'package:ignite_pay_app/services/direct_payment_service.dart';
import 'package:ignite_pay_app/services/phantom_wallet_service.dart';
import 'package:ignite_pay_app/cctp_transfer_screen.dart';
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
  try {
    await RustLib.init().timeout(const Duration(seconds: 10));
  } catch (e) {
    debugPrint('RustLib.init() failed: $e');
    // Still run the app so the user sees an error screen instead of a blank splash
  }

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
  String? _initError;
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
    try {
      final prefs = await SharedPreferences.getInstance();
      final hasDid = prefs.getBool('onboarding_complete') ?? false;

      if (hasDid) {
        // Load existing identity
        final svc = context.read<DidcommService>();
        await svc.initialize().timeout(const Duration(seconds: 15));
        // Auto-reconnect mediator if URL was previously saved
        final wsUrl = svc.mediatorWsUrl;
        if (wsUrl.isNotEmpty) {
          try {
            await svc.connectToMediator(wsUrl).timeout(const Duration(seconds: 10));
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
    } catch (e) {
      debugPrint('_checkOnboarding error: $e');
      if (mounted) {
        setState(() {
          _loading = false;
          _initError = e.toString();
        });
      }
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
    if (_initError != null) {
      return Scaffold(
        backgroundColor: _kBackground,
        body: Center(
          child: Padding(
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(LucideIcons.alertTriangle, size: 48, color: _kIntercepted),
                const SizedBox(height: 20),
                Text(
                  'Initialization Failed',
                  style: GoogleFonts.inter(
                    fontSize: 20,
                    fontWeight: FontWeight.w600,
                    color: _kTextPrimary,
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  _initError!,
                  style: GoogleFonts.inter(
                    fontSize: 12,
                    color: _kTextSecondary,
                  ),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 24),
                GestureDetector(
                  onTap: () {
                    setState(() { _initError = null; _loading = true; });
                    _checkOnboarding();
                  },
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
                    decoration: BoxDecoration(
                      gradient: const LinearGradient(colors: [_kNeonCyan, _kNeonCyanDim]),
                      borderRadius: BorderRadius.circular(10),
                    ),
                    child: Text(
                      'Retry',
                      style: GoogleFonts.inter(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: _kBackground,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    }
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
    if (uri.scheme != 'ignitepay') return;

    final path = '${uri.host}${uri.path}';

    // Route Phantom encrypted deep link callbacks to PhantomWalletService
    if (path == 'phantom/connect') {
      PhantomWalletService().handleConnectCallback(uri);
      return;
    }
    if (path == 'phantom/sign') {
      PhantomWalletService().handleSignCallback(uri);
      return;
    }
    if (path == 'phantom/signonly') {
      PhantomWalletService().handleSignOnlyCallback(uri);
      return;
    }

    switch (uri.host) {
      case 'onchain':
        // Existing session key registration callback
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
      case 'wallet_connect':
        // Direct wallet payment: connect callback
        final publicKey = uri.queryParameters['public_key'] ??
            uri.queryParameters['publicKey'];
        if (publicKey != null) {
          debugPrint('Wallet connect callback: publicKey=$publicKey');
          DirectPaymentService().handleConnectCallback(publicKey);
        }
      case 'direct_pay':
        // Direct wallet payment: sign-and-send callback
        final signature = uri.queryParameters['signature'];
        final errorCode = uri.queryParameters['errorCode'] ??
            uri.queryParameters['errorMessage'];
        debugPrint('Direct pay callback: signature=$signature, errorCode=$errorCode');
        DirectPaymentService().handlePaymentCallback(
          signature: signature,
          errorCode: errorCode,
        );
      case 'sponsored_sign':
        // Sponsored payment: signTransaction callback — wallet returns signed tx
        final transaction = uri.queryParameters['transaction'];
        final errorCode = uri.queryParameters['errorCode'] ??
            uri.queryParameters['errorMessage'];
        if (transaction != null) {
          debugPrint('Sponsored sign callback: transaction received');
          DirectPaymentService().handleSponsoredSignCallback(transaction);
        } else {
          debugPrint('Sponsored sign callback error: $errorCode');
          DirectPaymentService().handlePaymentCallback(
            signature: null,
            errorCode: errorCode ?? 'Sponsored signing failed',
          );
        }
      case 'cctp_approve':
        // CCTP: MetaMask approve callback — user returns after approving USDC
        debugPrint('CCTP approve callback received');
      case 'cctp_burn':
        // CCTP: MetaMask burn callback — user returns after depositForBurnWithHook
        // MetaMask deep links don't reliably return tx hash, so we rely on
        // the user entering it manually or polling starts from the burn tx hash
        debugPrint('CCTP burn callback received');
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
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    return SingleChildScrollView(
                      child: ConstrainedBox(
                        constraints: BoxConstraints(minHeight: constraints.maxHeight),
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            const DIDCard(),
                            const SizedBox(height: 20),
                            const _QuickNavRow(),
                            const SizedBox(height: 20),
                            const _SessionKeyBalanceCard(),
                            const SizedBox(height: 20),
                            const _RecentPaymentsPreview(),
                            const SizedBox(height: 24),
                            const _AuthAction(),
                          ],
                        ),
                      ),
                    );
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
                    'Devnet',
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
    return Column(
      children: [
        Row(
          children: [
            Expanded(
              child: _QuickNavCard(
                icon: LucideIcons.scanLine,
                label: 'Scan',
                subtitle: 'Scan to Pay',
                gradientColors: [const Color(0xFF00E5FF), const Color(0xFF0097A7)],
                onTap: () => _scanAndPay(context),
              ),
            ),
            const SizedBox(width: 10),
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
          ],
        ),
        const SizedBox(height: 10),
        _QuickNavCard(
          icon: LucideIcons.arrowDownToLine,
          label: 'Deposit',
          subtitle: 'Cross-chain USDC',
          gradientColors: [const Color(0xFF4CAF50), const Color(0xFF2E7D32)],
          onTap: () => openCctpTransfer(context),
        ),
      ],
    );
  }

  Future<void> _scanAndPay(BuildContext context) async {
    final didcomm = context.read<DidcommService>();
    final result = await showQrScanner(context);
    if (result == null || !context.mounted) return;

    if (result is PaymentQrData) {
      Navigator.of(context).push<dynamic>(
        MaterialPageRoute(
          builder: (_) => QrPaymentScreen(
            paymentData: result,
            storagePath: didcomm.storagePath,
            onConfirmPayment: ({
              required storagePath,
              required channelId,
              required hubEndpoint,
              required amount,
              required recipientPubkey,
            }) async {
              final channelSvc = ChannelService();
              await channelSvc.refreshChannels(storagePath);
              return channelSvc.channelPay(
                storagePath: storagePath,
                channelId: channelId,
                hubEndpoint: hubEndpoint,
                amount: amount,
                recipientPubkey: recipientPubkey,
              );
            },
          ),
        ),
      );
    }
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
// Session Key Balance Card
// ---------------------------------------------------------------------------
class _SessionKeyBalanceCard extends StatefulWidget {
  const _SessionKeyBalanceCard();

  @override
  State<_SessionKeyBalanceCard> createState() => _SessionKeyBalanceCardState();
}

class _SessionKeyBalanceCardState extends State<_SessionKeyBalanceCard> {
  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    final svc = SessionKeyService();
    if (svc.activeSessionKey != null) {
      await svc.refreshBalances();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<SessionKeyService>(
      builder: (context, svc, _) {
        final active = svc.activeSessionKey;

        if (active == null) {
          return GestureDetector(
            onTap: () => _openRecords(context),
            child: Container(
              width: double.infinity,
              padding: const EdgeInsets.all(20),
              decoration: BoxDecoration(
                color: _kSurfaceDark.withValues(alpha: 0.6),
                borderRadius: BorderRadius.circular(16),
                border: Border.all(color: _kGlassBorder),
              ),
              child: Column(
                children: [
                  Icon(LucideIcons.wallet, size: 28, color: _kTextSecondary.withValues(alpha: 0.4)),
                  const SizedBox(height: 10),
                  Text(
                    'No active session key',
                    style: GoogleFonts.inter(fontSize: 14, color: _kTextSecondary),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    'Authorize a payment to create one',
                    style: GoogleFonts.inter(fontSize: 11, color: _kTextSecondary.withValues(alpha: 0.6)),
                  ),
                ],
              ),
            ),
          );
        }

        final solBalance = svc.solBalance;
        final usdcBalance = svc.usdcBalance;
        final loading = svc.balanceLoading;

        return GestureDetector(
          onTap: () => _openRecords(context),
          child: Container(
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
                    Icon(LucideIcons.wallet, size: 16, color: _kNeonCyan.withValues(alpha: 0.8)),
                    const SizedBox(width: 8),
                    Text(
                      'SESSION KEY BALANCE',
                      style: GoogleFonts.inter(
                        fontSize: 11,
                        fontWeight: FontWeight.w600,
                        color: _kTextSecondary,
                        letterSpacing: 1.2,
                      ),
                    ),
                    const Spacer(),
                    if (loading)
                      SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: _kNeonCyan.withValues(alpha: 0.7),
                        ),
                      ),
                  ],
                ),
                const SizedBox(height: 16),
                Row(
                  children: [
                    Expanded(
                      child: _BalanceItem(
                        label: 'SOL',
                        value: (solBalance.toInt() / 1000000000.0).toStringAsFixed(4),
                        icon: LucideIcons.coins,
                      ),
                    ),
                    const SizedBox(width: 16),
                    Expanded(
                      child: _BalanceItem(
                        label: 'USDC',
                        value: (usdcBalance.toInt() / 1000000.0).toStringAsFixed(2),
                        icon: LucideIcons.dollarSign,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                Row(
                  children: [
                    Icon(
                      LucideIcons.keyRound,
                      size: 12,
                      color: _kTextSecondary.withValues(alpha: 0.5),
                    ),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        active.ephemeralPubkey.length > 20
                            ? '${active.ephemeralPubkey.substring(0, 8)}...${active.ephemeralPubkey.substring(active.ephemeralPubkey.length - 6)}'
                            : active.ephemeralPubkey,
                        style: GoogleFonts.jetBrainsMono(
                          fontSize: 11,
                          color: _kTextSecondary.withValues(alpha: 0.6),
                        ),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    Icon(
                      LucideIcons.chevronRight,
                      size: 14,
                      color: _kTextSecondary.withValues(alpha: 0.4),
                    ),
                  ],
                ),
              ],
            ),
          ),
        );
      },
    );
  }

  void _openRecords(BuildContext context) {
    Navigator.of(context).push<dynamic>(
      MaterialPageRoute(builder: (_) => const _PaymentRecordsScreen()),
    );
  }
}

class _BalanceItem extends StatelessWidget {
  final String label;
  final String value;
  final IconData icon;

  const _BalanceItem({
    required this.label,
    required this.value,
    required this.icon,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: _kGlassBorder),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(icon, size: 14, color: _kNeonCyan.withValues(alpha: 0.7)),
              const SizedBox(width: 6),
              Text(
                label,
                style: GoogleFonts.inter(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: _kTextSecondary,
                  letterSpacing: 0.8,
                ),
              ),
            ],
          ),
          const SizedBox(height: 6),
          Text(
            value,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 18,
              fontWeight: FontWeight.w600,
              color: _kTextPrimary,
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Recent Payments Preview
// ---------------------------------------------------------------------------
class _RecentPaymentsPreview extends StatefulWidget {
  const _RecentPaymentsPreview();

  @override
  State<_RecentPaymentsPreview> createState() => _RecentPaymentsPreviewState();
}

class _RecentPaymentsPreviewState extends State<_RecentPaymentsPreview> {
  List<session.PaymentRecord> _records = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _loadRecords();
  }

  Future<void> _loadRecords() async {
    final svc = SessionKeyService();
    final records = await svc.loadPaymentRecords();
    if (mounted) {
      setState(() {
        _records = records.take(3).toList();
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(LucideIcons.receipt, size: 16, color: _kNeonCyan.withValues(alpha: 0.8)),
            const SizedBox(width: 8),
            Text(
              'RECENT PAYMENTS',
              style: GoogleFonts.inter(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: _kTextSecondary,
                letterSpacing: 1.2,
              ),
            ),
            const Spacer(),
            if (_records.isNotEmpty)
              GestureDetector(
                onTap: () {
                  Navigator.of(context).push<dynamic>(
                    MaterialPageRoute(builder: (_) => const _PaymentRecordsScreen()),
                  );
                },
                child: Text(
                  'View all',
                  style: GoogleFonts.inter(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: _kNeonCyan,
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 12),
        if (_loading)
          const Center(
            child: Padding(
              padding: EdgeInsets.symmetric(vertical: 16),
              child: SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(strokeWidth: 2, color: _kNeonCyan),
              ),
            ),
          )
        else if (_records.isEmpty)
          Center(
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 20),
              child: Column(
                children: [
                  Icon(LucideIcons.inbox, size: 32, color: _kTextSecondary.withValues(alpha: 0.4)),
                  const SizedBox(height: 8),
                  Text(
                    'No payment records yet',
                    style: GoogleFonts.inter(fontSize: 13, color: _kTextSecondary),
                  ),
                ],
              ),
            ),
          )
        else
          ..._records.map((rec) {
            final isUsdc = rec.tokenMint != null &&
                (rec.tokenMint == 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v' ||
                    rec.tokenMint == '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU');
            final divisor = isUsdc ? 1000000.0 : 1000000000.0;
            final symbol = isUsdc ? 'USDC' : 'SOL';
            final amountStr = (rec.amount.toInt() / divisor).toStringAsFixed(isUsdc ? 2 : 4);
            final merchant = rec.merchantDid.length > 24
                ? '${rec.merchantDid.substring(0, 16)}...${rec.merchantDid.substring(rec.merchantDid.length - 6)}'
                : rec.merchantDid;
            final time = rec.timestamp > 0
                ? DateTime.fromMillisecondsSinceEpoch(rec.timestamp * 1000)
                : null;
            final timeStr = time != null
                ? '${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}'
                : '--';

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
                        color: (rec.authorized ? _kSuccess : _kIntercepted).withValues(alpha: 0.1),
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: Icon(
                        rec.authorized ? LucideIcons.checkCircle2 : LucideIcons.xCircle,
                        size: 18,
                        color: rec.authorized ? _kSuccess : _kIntercepted,
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            merchant,
                            style: GoogleFonts.inter(
                              fontSize: 13,
                              fontWeight: FontWeight.w600,
                              color: _kTextPrimary,
                            ),
                          ),
                          const SizedBox(height: 2),
                          Text(
                            timeStr,
                            style: GoogleFonts.inter(fontSize: 11, color: _kTextSecondary),
                          ),
                        ],
                      ),
                    ),
                    Column(
                      crossAxisAlignment: CrossAxisAlignment.end,
                      children: [
                        Text(
                          '$amountStr $symbol',
                          style: GoogleFonts.jetBrainsMono(
                            fontSize: 13,
                            fontWeight: FontWeight.w600,
                            color: _kTextPrimary,
                          ),
                        ),
                        const SizedBox(height: 4),
                        _StatusBadge(
                          color: rec.authorized ? _kSuccess : _kIntercepted,
                          label: rec.authorized ? 'Success' : 'Declined',
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            );
          }),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Payment Records Screen (full-screen)
// ---------------------------------------------------------------------------
class _PaymentRecordsScreen extends StatefulWidget {
  const _PaymentRecordsScreen();

  @override
  State<_PaymentRecordsScreen> createState() => _PaymentRecordsScreenState();
}

class _PaymentRecordsScreenState extends State<_PaymentRecordsScreen> {
  List<session.PaymentRecord> _records = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _loadRecords();
  }

  Future<void> _loadRecords() async {
    final svc = SessionKeyService();
    final records = await svc.loadPaymentRecords();
    if (mounted) {
      setState(() {
        _records = records;
        _loading = false;
      });
    }
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    await _loadRecords();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: _kBackground,
      appBar: AppBar(
        backgroundColor: _kSurfaceDark,
        title: Text(
          'Payment Records',
          style: GoogleFonts.inter(
            fontSize: 18,
            fontWeight: FontWeight.w700,
            color: _kTextPrimary,
          ),
        ),
        iconTheme: const IconThemeData(color: _kTextPrimary),
        elevation: 0,
      ),
      body: _loading
          ? const Center(
              child: CircularProgressIndicator(color: _kNeonCyan),
            )
          : _records.isEmpty
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(LucideIcons.inbox, size: 48, color: _kTextSecondary.withValues(alpha: 0.4)),
                      const SizedBox(height: 12),
                      Text(
                        'No payment records',
                        style: GoogleFonts.inter(fontSize: 16, color: _kTextSecondary),
                      ),
                    ],
                  ),
                )
              : RefreshIndicator(
                  color: _kNeonCyan,
                  onRefresh: _refresh,
                  child: ListView.builder(
                    padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
                    itemCount: _records.length,
                    itemBuilder: (context, index) {
                      final rec = _records[index];
                      final isUsdc = rec.tokenMint != null &&
                          (rec.tokenMint == 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v' ||
                              rec.tokenMint == '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU');
                      final divisor = isUsdc ? 1000000.0 : 1000000000.0;
                      final symbol = isUsdc ? 'USDC' : 'SOL';
                      final amountStr = (rec.amount.toInt() / divisor).toStringAsFixed(isUsdc ? 2 : 4);
                      final merchant = rec.merchantDid.length > 24
                          ? '${rec.merchantDid.substring(0, 16)}...${rec.merchantDid.substring(rec.merchantDid.length - 6)}'
                          : rec.merchantDid;
                      final time = rec.timestamp > 0
                          ? DateTime.fromMillisecondsSinceEpoch(rec.timestamp * 1000)
                          : null;
                      final dateStr = time != null
                          ? '${time.month}/${time.day} ${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}'
                          : '--';

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
                                  color: (rec.authorized ? _kSuccess : _kIntercepted)
                                      .withValues(alpha: 0.1),
                                  borderRadius: BorderRadius.circular(10),
                                ),
                                child: Icon(
                                  rec.authorized ? LucideIcons.checkCircle2 : LucideIcons.xCircle,
                                  size: 18,
                                  color: rec.authorized ? _kSuccess : _kIntercepted,
                                ),
                              ),
                              const SizedBox(width: 12),
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      merchant,
                                      style: GoogleFonts.inter(
                                        fontSize: 13,
                                        fontWeight: FontWeight.w600,
                                        color: _kTextPrimary,
                                      ),
                                    ),
                                    const SizedBox(height: 2),
                                    Text(
                                      rec.description.isNotEmpty ? rec.description : dateStr,
                                      style: GoogleFonts.inter(fontSize: 11, color: _kTextSecondary),
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ],
                                ),
                              ),
                              Column(
                                crossAxisAlignment: CrossAxisAlignment.end,
                                children: [
                                  Text(
                                    '$amountStr $symbol',
                                    style: GoogleFonts.jetBrainsMono(
                                      fontSize: 13,
                                      fontWeight: FontWeight.w600,
                                      color: _kTextPrimary,
                                    ),
                                  ),
                                  const SizedBox(height: 4),
                                  _StatusBadge(
                                    color: rec.authorized ? _kSuccess : _kIntercepted,
                                    label: rec.authorized ? 'Success' : 'Declined',
                                  ),
                                ],
                              ),
                            ],
                          ),
                        ),
                      );
                    },
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
