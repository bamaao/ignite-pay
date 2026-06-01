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

import 'dart:async';
import 'dart:ui';
import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:lucide_icons/lucide_icons.dart';
import 'package:ignite_pay_app/services/app_log_service.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
import 'package:ignite_pay_app/services/wallet_picker.dart';
import 'package:ignite_pay_app/services/wallet_service.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as rust;
import 'package:ignite_pay_app/src/rust/api/session.dart' as session;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

// Re-export MerchantProfile from generated bindings for convenience.
typedef MerchantProfile = rust.MerchantProfile;

// ---------------------------------------------------------------------------
// Challenge Theme
// ---------------------------------------------------------------------------
const _kAmber = Color(0xFFFFB300);
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
  bool _checkingExistingKey = true;
  bool _wizardLaunched = false;

  // Authorization policy fields (editable by user)
  String _dailySpendingLimit = ''; // SOL string, default = amount*10
  String _dailyTxCountLimit = '50';
  String _perTxLimit = ''; // SOL string, default = amount
  String _durationHours = '24';

  // Funding fields — visible only when creating a new session key
  String _solFundingAmount = '0.01'; // SOL
  String _usdcFundingAmount = '1.0'; // USDC

  // Wallet service — mobile defaults to Phantom deep link (no QR)
  WalletService? _walletService;

  // On-chain PDA existence check
  bool _pdaExistsOnChain = false;

  /// Whether this is a first-time session key creation (PDA not found on-chain).
  bool get _isNewSessionKey => !_pdaExistsOnChain;

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
    // Set default policy values based on payment amount
    final solAmount = _amount / 1000000000.0;
    _dailySpendingLimit = (solAmount * 10).toStringAsFixed(2);
    _perTxLimit = solAmount.toStringAsFixed(2);
    _checkExistingSessionKey();
  }

  @override
  void dispose() {
    _glowCtrl.dispose();
    super.dispose();
  }

  Future<void> _checkExistingSessionKey() async {
    final svc = SessionKeyService();
    await svc.initialize();
    final existing = await svc.checkExistingKey();

    // Load saved merchant policy if available
    try {
      final dir = await getApplicationSupportDirectory();
      if (_merchantDid.isNotEmpty) {
        final policy = await rust.loadMerchantPolicy(
          storagePath: dir.path,
          merchantDid: _merchantDid,
        );
        if (policy != null && mounted) {
          setState(() {
            _dailySpendingLimit = (policy.dailySpendingLimit.toInt() / 1000000000.0).toStringAsFixed(2);
            _dailyTxCountLimit = policy.dailyTxCountLimit.toString();
            _perTxLimit = (policy.perTxLimit.toInt() / 1000000000.0).toStringAsFixed(2);
            _durationHours = (policy.durationSecs / 3600).round().toString();
          });
        } else if (mounted) {
          // Use MCP-suggested values as defaults if no saved policy
          final req = widget.request;
          if (req != null) {
            setState(() {
              if (req.suggestedPerTxLimit != null && req.suggestedPerTxLimit! > 0) {
                _perTxLimit = (req.suggestedPerTxLimit! / 1000000000.0).toStringAsFixed(2);
              }
              if (req.suggestedDailyTxCountLimit != null && req.suggestedDailyTxCountLimit! > 0) {
                _dailyTxCountLimit = req.suggestedDailyTxCountLimit.toString();
              }
            });
          }
        }
      }
    } catch (_) {
      // No saved policy — keep defaults
    }

    // Check on-chain PDA existence
    if (existing != null) {
      final onChainInfo = await svc.checkOnChainExists();
      if (mounted) {
        setState(() {
          _pdaExistsOnChain = onChainInfo?.exists ?? false;
        });
      }
    }

    if (mounted) {
      setState(() {
        _checkingExistingKey = false;
      });
      if (existing != null && _pdaExistsOnChain) {
        setState(() {
          _authResult = 'Using existing session key';
        });
      } else {
        // New session key — initialize funding defaults from MCP suggestions
        final req = widget.request;
        if (req != null) {
          setState(() {
            if (req.newSessionKeySuggestedSolFunding != null && req.newSessionKeySuggestedSolFunding! > 0) {
              _solFundingAmount = (req.newSessionKeySuggestedSolFunding! / 1000000000.0).toStringAsFixed(4);
            }
            if (req.newSessionKeySuggestedTokenFunding != null && req.newSessionKeySuggestedTokenFunding! > 0) {
              _usdcFundingAmount = (req.newSessionKeySuggestedTokenFunding! / 1000000.0).toStringAsFixed(2);
            }
          });
        }
        // Auto-open wizard when PDA doesn't exist
        if (!_wizardLaunched && _isNewSessionKey) {
          _wizardLaunched = true;
          WidgetsBinding.instance.addPostFrameCallback((_) => _openPdaWizard());
        }
      }
    }
  }

  Future<void> _openPdaWizard() async {
    final result = await Navigator.of(context).push<String>(
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
            child: _PdaSetupWizard(
              request: widget.request,
              dailySpendingLimit: _dailySpendingLimit,
              dailyTxCountLimit: _dailyTxCountLimit,
              perTxLimit: _perTxLimit,
              durationHours: _durationHours,
              solFundingAmount: _solFundingAmount,
              usdcFundingAmount: _usdcFundingAmount,
              walletService: _walletService,
              listAction: _listAction,
              listLabel: _listLabel,
              listMaxAmount: _listMaxAmount,
            ),
          );
        },
      ),
    );

    if (!mounted) return;

    if (result == 'authorized') {
      Navigator.of(context).pop('authorized');
    } else if (result == 'declined') {
      Navigator.of(context).pop('declined');
    }
    // If wizard was dismissed without action, stay on challenge screen
  }

  Future<void> _onAuthorize() async {
    final svc = SessionKeyService();

    // F2: If MCP provided a new session key, handle auth flow
    final mcpSessionKey = widget.request?.newSessionKeyPubkey;
    AppLogService().info('Auth', 'onAuthorize: newSessionKeyPubkey=$mcpSessionKey, tokenMint=${widget.request?.tokenMint}, hasRequest=${widget.request != null}');
    if (mcpSessionKey != null && mcpSessionKey.isNotEmpty) {
      setState(() {
        _isAuthorizing = true;
        _authResult = 'Authorizing...';
      });
      try {
        final req = widget.request!;
        await svc.initialize();

        final dir = await getApplicationSupportDirectory();

        // ── Wallet-free path: PDA already exists on-chain ──
        // Only needs cached public key from SharedPreferences, no active wallet connection.
        if (_pdaExistsOnChain) {
          final wallet = _walletService ?? createDefaultWalletService();
          await wallet.loadSession(); // loads cached public key
          final cachedPubKey = wallet.walletPublicKey;
          if (cachedPubKey == null) throw 'No cached wallet key — connect wallet first';

          setState(() => _authResult = 'Finalizing session key...');
          final onChainInfo = await rust.getSessionAccountInfo(
            rpcUrl: svc.rpcUrl,
            ownerB58: cachedPubKey,
            ephemeralB58: req.newSessionKeyPubkey!,
          );

          if (onChainInfo.exists && !onChainInfo.revoked) {
            final scopes = req.newSessionKeyScopes ?? ['sol:transfer', 'spl:transfer'];
            final info = await rust.finalizeExistingSessionKey(
              storagePath: dir.path,
              ownerPubkeyB58: cachedPubKey,
              ephemeralPubkey: req.newSessionKeyPubkey!,
              onChainInfo: onChainInfo,
              scopes: scopes,
            );

            await _sendAuthResponseWithExternalKey(info);
            setState(() => _authResult = 'Authorized');
            await Future.delayed(const Duration(milliseconds: 1200));
            if (mounted) Navigator.of(context).pop('authorized');
            return;
          }
          // PDA no longer exists despite _pdaExistsOnChain — fall through to full wallet flow
          AppLogService().warn('Auth', 'PDA marked as existing but on-chain check failed, falling through to wallet flow');
        }

        // ── If PDA doesn't exist, this should have been handled by wizard ──
        // If we reach here, the wizard was dismissed without completing.
        setState(() {
          _authResult = 'Error: Session key not registered — setup wizard required';
          _isAuthorizing = false;
        });
      } catch (e) {
        setState(() {
          _authResult = 'Error: $e';
          _isAuthorizing = false;
        });
      }
      return;
    }

    // If an active key already exists, skip creation
    AppLogService().info('Auth', 'No MCP session key, checking existing: activeSessionKey=${svc.activeSessionKey}');
    if (svc.activeSessionKey != null) {
      setState(() {
        _isAuthorizing = true;
        _authResult = 'Using existing session key...';
      });
      try {
        await _sendAuthResponse();
        setState(() => _authResult = 'Authorized with existing session key');
        await Future.delayed(const Duration(milliseconds: 1200));
        if (mounted) Navigator.of(context).pop('authorized');
      } catch (e) {
        setState(() {
          _authResult = 'Error: $e';
          _isAuthorizing = false;
        });
      }
      return;
    }

    // No MCP session key provided and no existing key — MCP should always provide
    // an ephemeral keypair when a new session key is needed. Show error.
    setState(() {
      _authResult = 'Error: MCP server did not provide a session key';
    });
  }

  Future<void> _sendAuthResponse() async {
    // Parse policy values from inputs
    final dailyLimitLamports = (_parseSol(_dailySpendingLimit) * 1000000000).round();
    final perTxLimitLamports = (_parseSol(_perTxLimit) * 1000000000).round();
    final dailyTxCount = int.tryParse(_dailyTxCountLimit) ?? 50;
    final durationSecs = (int.tryParse(_durationHours) ?? 24) * 3600;

    // Persist merchant policy to sled
    try {
      final dir = await getApplicationSupportDirectory();
      if (_merchantDid.isNotEmpty) {
        await rust.saveMerchantPolicy(
          storagePath: dir.path,
          merchantDid: _merchantDid,
          dailySpendingLimit: BigInt.from(dailyLimitLamports),
          dailyTxCountLimit: dailyTxCount,
          perTxLimit: BigInt.from(perTxLimitLamports),
          durationSecs: durationSecs,
        );

        // Track this merchant DID for policy screen
        final prefs = await SharedPreferences.getInstance();
        final known = prefs.getStringList('known_merchant_dids') ?? [];
        if (!known.contains(_merchantDid)) {
          known.add(_merchantDid);
          await prefs.setStringList('known_merchant_dids', known);
        }
      }
    } catch (e) {
      debugPrint('Failed to save merchant policy: $e');
    }

    await DidcommService().sendAuthResponseWithSessionKey(
      paymentId: _paymentId,
      authorized: true,
      listAction: _listAction,
      spendingLimit: dailyLimitLamports,
      durationSecs: durationSecs,
      listLabel: _showLabelInput && _listLabel.isNotEmpty ? _listLabel : null,
      listMaxAmount: _showMaxAmountInput && _listMaxAmount.isNotEmpty
          ? int.tryParse(_listMaxAmount)
          : null,
      dailyTxCountLimit: dailyTxCount,
      perTxLimit: perTxLimitLamports,
    );

    // Save payment record
    final svc = SessionKeyService();
    await svc.savePaymentRecord(
      paymentId: _paymentId,
      merchantDid: _merchantDid,
      amount: BigInt.from(_amount),
      tokenMint: widget.request?.tokenMint,
      description: _description,
      authorized: true,
      sessionKeyPubkey: svc.activeSessionKey?.ephemeralPubkey,
    );
  }

  /// Send auth response with externally-provided session key info.
  Future<void> _sendAuthResponseWithExternalKey(session.SessionKeyInfo info) async {
    // Parse policy values from inputs
    final dailyLimitLamports = (_parseSol(_dailySpendingLimit) * 1000000000).round();
    final perTxLimitLamports = (_parseSol(_perTxLimit) * 1000000000).round();
    final dailyTxCount = int.tryParse(_dailyTxCountLimit) ?? 50;
    final durationSecs = (int.tryParse(_durationHours) ?? 24) * 3600;

    // Persist merchant policy
    try {
      final dir = await getApplicationSupportDirectory();
      if (_merchantDid.isNotEmpty) {
        await rust.saveMerchantPolicy(
          storagePath: dir.path,
          merchantDid: _merchantDid,
          dailySpendingLimit: BigInt.from(dailyLimitLamports),
          dailyTxCountLimit: dailyTxCount,
          perTxLimit: BigInt.from(perTxLimitLamports),
          durationSecs: durationSecs,
        );

        final prefs = await SharedPreferences.getInstance();
        final known = prefs.getStringList('known_merchant_dids') ?? [];
        if (!known.contains(_merchantDid)) {
          known.add(_merchantDid);
          await prefs.setStringList('known_merchant_dids', known);
        }
      }
    } catch (e) {
      debugPrint('Failed to save merchant policy: $e');
    }

    await rust.sendAuthResponse(
      storagePath: DidcommService().storagePath,
      paymentId: _paymentId,
      authorized: true,
      listAction: _listAction,
      mcpDid: DidcommService().pairedMcps.isNotEmpty
          ? DidcommService().pairedMcps.first.did
          : '',
      sessionKeyInfo: info,
      listLabel: _showLabelInput && _listLabel.isNotEmpty ? _listLabel : null,
      listMaxAmount: _showMaxAmountInput && _listMaxAmount.isNotEmpty
          ? int.tryParse(_listMaxAmount) != null
              ? BigInt.from(int.parse(_listMaxAmount))
              : null
          : null,
      dailyTxCountLimit: dailyTxCount,
      perTxLimit: BigInt.from(perTxLimitLamports),
      tokenMint: widget.request?.newSessionKeyTokenMint,
      paymentMethod: null,
    );

    DidcommService().clearPendingAuth();

    // Save payment record
    final svc = SessionKeyService();
    await svc.savePaymentRecord(
      paymentId: _paymentId,
      merchantDid: _merchantDid,
      amount: BigInt.from(_amount),
      tokenMint: widget.request?.tokenMint ?? info.sessionPda,
      description: _description,
      authorized: true,
      sessionKeyPubkey: info.ephemeralPubkey,
      txSignature: info.txSignature,
    );
  }

  /// Parse a SOL string to a double, returning 0 on failure.
  double _parseSol(String value) {
    return double.tryParse(value) ?? 0.0;
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
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    const SizedBox(height: 16),
                    const _ChallengeHeader(),
                    const SizedBox(height: 20),
                    _MerchantCard(merchantDid: _merchantDid),
                    const SizedBox(height: 20),
                    _AmountDisplay(amount: _amount, tokenMint: widget.request?.tokenMint),
                    const SizedBox(height: 8),
                    _ReasonBlock(description: _description),
                    const SizedBox(height: 16),
                    _AuthorizationPolicyCard(
                      dailySpendingLimit: _dailySpendingLimit,
                      onDailySpendingLimitChanged: (v) => setState(() => _dailySpendingLimit = v),
                      dailyTxCountLimit: _dailyTxCountLimit,
                      onDailyTxCountLimitChanged: (v) => setState(() => _dailyTxCountLimit = v),
                      perTxLimit: _perTxLimit,
                      onPerTxLimitChanged: (v) => setState(() => _perTxLimit = v),
                      durationHours: _durationHours,
                      onDurationHoursChanged: (v) => setState(() => _durationHours = v),
                    ),
                    const SizedBox(height: 16),
                    // If wizard is being launched, show loading indicator
                    if (_isNewSessionKey && !_checkingExistingKey) ...[
                      const Padding(
                        padding: EdgeInsets.symmetric(vertical: 16),
                        child: Center(
                          child: Column(
                            children: [
                              SizedBox(
                                width: 24,
                                height: 24,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                  color: _kAmber,
                                ),
                              ),
                              SizedBox(height: 8),
                              Text(
                                'Opening setup wizard...',
                                style: TextStyle(color: _kTextSecondary, fontSize: 12),
                              ),
                            ],
                          ),
                        ),
                      ),
                      const SizedBox(height: 8),
                    ],
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
                    if (_authResult.isNotEmpty) ...[
                      const SizedBox(height: 12),
                      _ResultBanner(result: _authResult),
                    ],
                    if (_checkingExistingKey)
                      const Padding(
                        padding: EdgeInsets.only(top: 12),
                        child: Center(
                          child: SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: _kAmber,
                            ),
                          ),
                        ),
                      ),
                    const SizedBox(height: 20),
                    _ApproveButton(
                      onTap: _isAuthorizing ? null : _onAuthorize,
                      isAuthorizing: _isAuthorizing,
                    ),
                    const SizedBox(height: 10),
                    _DeclineButton(onTap: _onDecline),
                    const SizedBox(height: 32),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// PDA Setup Wizard — 4-step flow for new session key creation + funding
// ---------------------------------------------------------------------------
class _PdaSetupWizard extends StatefulWidget {
  final AuthRequest? request;
  final String dailySpendingLimit;
  final String dailyTxCountLimit;
  final String perTxLimit;
  final String durationHours;
  final String solFundingAmount;
  final String usdcFundingAmount;
  final WalletService? walletService;
  final String listAction;
  final String listLabel;
  final String listMaxAmount;

  const _PdaSetupWizard({
    this.request,
    required this.dailySpendingLimit,
    required this.dailyTxCountLimit,
    required this.perTxLimit,
    required this.durationHours,
    required this.solFundingAmount,
    required this.usdcFundingAmount,
    this.walletService,
    required this.listAction,
    required this.listLabel,
    required this.listMaxAmount,
  });

  @override
  State<_PdaSetupWizard> createState() => _PdaSetupWizardState();
}

class _PdaSetupWizardState extends State<_PdaSetupWizard>
    with SingleTickerProviderStateMixin {
  late final AnimationController _glowCtrl;
  int _currentStep = 0; // 0=Create PDA, 1=Fund SOL, 2=Fund USDC, 3=Authorize
  String _statusText = '';
  bool _isBusy = false;
  String? _errorMessage;

  // Policy fields (editable in step 0)
  late String _dailySpendingLimit;
  late String _dailyTxCountLimit;
  late String _perTxLimit;
  late String _durationHours;

  // Funding fields (editable in steps 1-2)
  late String _solFundingAmount;
  late String _usdcFundingAmount;

  // List action (step 3)
  late String _listAction;
  late String _listLabel;
  late String _listMaxAmount;

  // Session info after step 0
  session.SessionKeyInfo? _sessionKeyInfo;
  String? _sessionPda;
  String? _ephemeralPubkey;

  /// Wizard-local wallet (Phantom on mobile by default).
  WalletService? _wallet;

  WalletService _resolveWallet() =>
      widget.walletService ?? _wallet ?? createDefaultWalletService();

  String _walletLabel(WalletService wallet) => wallet.walletDisplayName;

  Future<void> _switchWallet() async {
    if (_isBusy) return;
    final picked = await selectWalletService(context);
    if (_wallet != null && _wallet!.isConnected) {
      await _wallet!.disconnect();
    }
    setState(() => _wallet = picked);
  }

  Future<WalletService> _connectWallet() async {
    final wallet = _resolveWallet();
    await ensureWalletConnected(
      wallet,
      Navigator.of(context, rootNavigator: true).context,
    );
    return wallet;
  }

  String get _merchantDid => widget.request?.merchantDid ?? '';
  int get _amount => widget.request?.amount ?? 0;
  String get _paymentId => widget.request?.paymentId ?? '';
  String get _description => widget.request?.description ?? '';

  bool get _showLabelInput => _listAction == 'add_whitelist' || _listAction == 'add_blacklist';
  bool get _showMaxAmountInput => _listAction == 'add_whitelist';

  static const _stepIcons = [
    LucideIcons.keyRound,
    LucideIcons.coins,
    LucideIcons.banknote,
    LucideIcons.shieldCheck,
  ];
  static const _stepTitles = [
    'Create Session Key',
    'Fund SOL (Gas)',
    'Fund USDC',
    'Authorize Payment',
  ];

  @override
  void initState() {
    super.initState();
    _glowCtrl = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 2000),
    )..repeat(reverse: true);
    _dailySpendingLimit = widget.dailySpendingLimit;
    _dailyTxCountLimit = widget.dailyTxCountLimit;
    _perTxLimit = widget.perTxLimit;
    _durationHours = widget.durationHours;
    _solFundingAmount = widget.solFundingAmount;
    _usdcFundingAmount = widget.usdcFundingAmount;
    _listAction = widget.listAction;
    _listLabel = widget.listLabel;
    _listMaxAmount = widget.listMaxAmount;
  }

  @override
  void dispose() {
    _glowCtrl.dispose();
    super.dispose();
  }

  double _parseSol(String value) => double.tryParse(value) ?? 0.0;

  bool _isMethodUnsupported(String? err) {
    final s = (err ?? '').toLowerCase();
    return s.contains('not supported') ||
        s.contains('unsupported') ||
        s.contains('method is not supported');
  }

  Future<String> _sendSolFundingTx({
    required WalletService wallet,
    required String unsignedTxB58,
    required String rpcUrl,
  }) async {
    if (mounted) {
      setState(() => _statusText = 'Open wallet to sign SOL transaction...');
    }
    final signedTx = await wallet.signTransaction(unsignedTxB58);
    if (signedTx != null && signedTx.isNotEmpty) {
      if (mounted) {
        setState(() => _statusText = 'Broadcasting signed SOL transaction...');
      }
      return session.broadcastSignedTx(rpcUrl: rpcUrl, signedTxB58: signedTx);
    }

    final signErr = wallet.lastError;
    if (!_isMethodUnsupported(signErr)) {
      throw signErr != null && signErr.isNotEmpty
          ? 'Wallet rejected SOL transfer: $signErr'
          : 'Wallet rejected SOL transfer';
    }

    if (mounted) {
      setState(() => _statusText = 'sign-only not supported, trying sign-and-send...');
    }
    final viaSignAndSend = await wallet.signAndSendTransaction(unsignedTxB58);
    if (viaSignAndSend != null && viaSignAndSend.isNotEmpty) {
      return viaSignAndSend;
    }
    final sendErr = wallet.lastError;
    throw sendErr != null && sendErr.isNotEmpty
        ? 'Wallet does not support both signTransaction and signAndSendTransaction: $sendErr'
        : 'Wallet does not support both signTransaction and signAndSendTransaction';
  }

  Future<String> _sendUsdcFundingTx({
    required WalletService wallet,
    required String unsignedTxB58,
    required String rpcUrl,
  }) async {
    if (mounted) {
      setState(() => _statusText = 'Open wallet to sign USDC transaction...');
    }
    final signedTx = await wallet.signTransaction(unsignedTxB58);
    if (signedTx != null && signedTx.isNotEmpty) {
      if (mounted) {
        setState(() => _statusText = 'Broadcasting signed USDC transaction...');
      }
      return session.broadcastSignedTx(rpcUrl: rpcUrl, signedTxB58: signedTx);
    }

    final signErr = wallet.lastError;
    if (!_isMethodUnsupported(signErr)) {
      throw signErr != null && signErr.isNotEmpty
          ? 'Wallet rejected USDC transfer: $signErr'
          : 'Wallet rejected USDC transfer';
    }

    if (mounted) {
      setState(() => _statusText = 'sign-only not supported, trying sign-and-send...');
    }
    // Avoid two deep-link launches back-to-back causing wallet app instability.
    await Future.delayed(const Duration(milliseconds: 450));
    final viaSignAndSend = await wallet.signAndSendTransaction(unsignedTxB58);
    if (viaSignAndSend != null && viaSignAndSend.isNotEmpty) {
      return viaSignAndSend;
    }
    final sendErr = wallet.lastError;
    throw sendErr != null && sendErr.isNotEmpty
        ? 'Wallet does not support both signTransaction and signAndSendTransaction: $sendErr'
        : 'Wallet does not support both signTransaction and signAndSendTransaction';
  }

  Future<bool> _waitForSessionPdaConfirmed({
    required SessionKeyService svc,
    required WalletService wallet,
    required String ephemeralPubkey,
  }) async {
    const maxAttempts = 12; // ~24s
    for (var i = 0; i < maxAttempts; i++) {
      try {
        final onChainInfo = await rust.getSessionAccountInfo(
          rpcUrl: svc.rpcUrl,
          ownerB58: wallet.walletPublicKey!,
          ephemeralB58: ephemeralPubkey,
        );
        if (onChainInfo.exists && !onChainInfo.revoked) {
          return true;
        }
      } catch (_) {
        // Retry transient RPC/read errors.
      }
      if (mounted) {
        setState(() => _statusText = 'Waiting for on-chain confirmation... (${i + 1}/$maxAttempts)');
      }
      await Future.delayed(const Duration(seconds: 2));
    }
    return false;
  }

  // ── Step 0: Create PDA ────────────────────────────────────────────────
  Future<void> _onCreatePda() async {
    if (_isBusy) return;
    setState(() {
      _isBusy = true;
      _errorMessage = null;
      _statusText = 'Connecting to wallet...';
    });

    try {
      final req = widget.request!;
      final svc = SessionKeyService();
      await svc.initialize();
      final dir = await getApplicationSupportDirectory();

      // 1. Connect wallet (Phantom deep link on mobile — opens installed app)
      final wallet = await _connectWallet();

      // 1.1 If this MCP-provided session key is already on-chain, reuse it.
      setState(() => _statusText = 'Checking existing session PDA on-chain...');
      final existingOnChain = await rust.getSessionAccountInfo(
        rpcUrl: svc.rpcUrl,
        ownerB58: wallet.walletPublicKey!,
        ephemeralB58: req.newSessionKeyPubkey!,
      );
      if (existingOnChain.exists && !existingOnChain.revoked) {
        final info = await rust.finalizeExistingSessionKey(
          storagePath: dir.path,
          ownerPubkeyB58: wallet.walletPublicKey!,
          ephemeralPubkey: req.newSessionKeyPubkey!,
          onChainInfo: existingOnChain,
          scopes: req.newSessionKeyScopes ?? ['sol:transfer', 'spl:transfer'],
        );
        setState(() {
          _sessionKeyInfo = info;
          _sessionPda = info.sessionPda;
          _ephemeralPubkey = info.ephemeralPubkey;
          _statusText = 'Session key already exists on-chain, reusing.';
          _currentStep = 1;
          _isBusy = false;
        });
        return;
      }

      // 2. Build register tx
      setState(() => _statusText = 'Building register transaction...');
      final isSpl = (req.newSessionKeyScopes ?? []).any((s) => s.contains('spl'));
      final targetProgram = isSpl
          ? 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'
          : '11111111111111111111111111111111';

      final unsignedRegister = await session.buildRegisterTxForPhantom(
        storagePath: dir.path,
        rpcUrl: svc.rpcUrl,
        ownerPubkeyB58: wallet.walletPublicKey!,
        ephemeralPubkeyB58: req.newSessionKeyPubkey!,
        targetProgram: targetProgram,
        scopes: req.newSessionKeyScopes ?? ['sol:transfer', 'spl:transfer'],
        spendingLimit: BigInt.from(req.newSessionKeySpendingLimit ?? 0),
        durationSecs: req.newSessionKeyDurationSecs ?? 3600,
        perTxLimit: BigInt.from((_parseSol(_perTxLimit) * 1000000000).round()),
        dailyTxCountLimit: int.tryParse(_dailyTxCountLimit) ?? 50,
        tokenMint: req.newSessionKeyTokenMint,
      );

      // 3. Wallet sign (sign only, not broadcast)
      setState(() => _statusText = 'Open wallet to sign register tx...');
      final signedRegisterTx = await wallet.signTransaction(unsignedRegister.unsignedTxB58);
      String registerSig;
      if (signedRegisterTx == null) {
        // Fallback: some wallets reject sign-only but accept sign-and-send.
        setState(() => _statusText = 'signTransaction failed, trying sign-and-send...');
        final onchainSig = await wallet.signAndSendTransaction(unsignedRegister.unsignedTxB58);
        if (onchainSig == null) {
          final detail = wallet.lastError;
          if (_isMethodUnsupported(detail)) {
            throw 'Wallet does not support both signTransaction and signAndSendTransaction';
          }
          throw detail != null && detail.isNotEmpty
              ? 'Wallet rejected register transaction: $detail'
              : 'Wallet rejected register transaction';
        }
        registerSig = onchainSig;
      } else {
        // 4. Broadcast
        setState(() => _statusText = 'Broadcasting register transaction...');
        registerSig = await session.broadcastSignedTx(
          rpcUrl: svc.rpcUrl,
          signedTxB58: signedRegisterTx,
        );
      }

      // 5. Finalize
      final info = await session.finalizePhantomSessionKey(
        storagePath: dir.path,
        ephemeralPubkey: unsignedRegister.ephemeralPubkey,
        txSignature: registerSig,
        sessionPda: unsignedRegister.sessionPda,
      );

      // Guard against false positives: signature can exist even if tx failed.
      final confirmed = await _waitForSessionPdaConfirmed(
        svc: svc,
        wallet: wallet,
        ephemeralPubkey: unsignedRegister.ephemeralPubkey,
      );
      if (!confirmed) {
        throw 'Session PDA registration is still pending on-chain. Please retry Create PDA in a few seconds.';
      }

      setState(() {
        _sessionKeyInfo = info;
        _sessionPda = info.sessionPda ?? unsignedRegister.sessionPda;
        _ephemeralPubkey = info.ephemeralPubkey;
        _statusText = 'Session key created!';
        _currentStep = 1;
        _isBusy = false;
      });
    } catch (e) {
      setState(() {
        _errorMessage = e.toString();
        _statusText = '';
        _isBusy = false;
      });
    }
  }

  // ── Step 1: Fund SOL (gas) ────────────────────────────────────────────
  Future<void> _onFundSol() async {
    if (_isBusy) return;
    setState(() {
      _isBusy = true;
      _errorMessage = null;
      _statusText = 'Connecting to wallet...';
    });

    try {
      final svc = SessionKeyService();
      await svc.initialize();
      final wallet = await _connectWallet();

      final solAmount = _parseSol(_solFundingAmount);
      if (solAmount <= 0) throw 'Enter a valid SOL amount';

      // Send SOL to ephemeral key for gas
      setState(() => _statusText = 'Open wallet to send SOL...');
      final solLamports = (solAmount * 1000000000).round();
      final txB58 = await session.buildUnsignedTransferTx(
        rpcUrl: svc.rpcUrl,
        walletPubkeyB58: wallet.walletPublicKey!,
        merchantDid: _ephemeralPubkey!,
        amountLamports: BigInt.from(solLamports),
      );
      await _sendSolFundingTx(
        wallet: wallet,
        unsignedTxB58: txB58,
        rpcUrl: svc.rpcUrl,
      );

      setState(() {
        _statusText = 'SOL sent!';
        _currentStep = 2;
        _isBusy = false;
      });
    } catch (e) {
      setState(() {
        _errorMessage = e.toString();
        _statusText = '';
        _isBusy = false;
      });
    }
  }

  // ── Step 2: Fund USDC ─────────────────────────────────────────────────
  Future<void> _onFundUsdc() async {
    if (_isBusy) return;
    setState(() {
      _isBusy = true;
      _errorMessage = null;
      _statusText = 'Connecting to wallet...';
    });

    try {
      final svc = SessionKeyService();
      await svc.initialize();
      final wallet = await _connectWallet();

      final usdcAmount = double.tryParse(_usdcFundingAmount) ?? 0.0;
      if (usdcAmount <= 0) throw 'Enter a valid USDC amount';

      final tokenMint = widget.request?.newSessionKeyTokenMint
          ?? '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU'; // devnet USDC

      setState(() => _statusText = 'Open wallet to send USDC...');
      final usdcRaw = (usdcAmount * 1000000).round();
      final txB58 = await session.buildUnsignedSplTransferTx(
        rpcUrl: svc.rpcUrl,
        walletPubkeyB58: wallet.walletPublicKey!,
        merchantWalletB58: _sessionPda!,
        amount: BigInt.from(usdcRaw),
        tokenMintB58: tokenMint,
      );
      await _sendUsdcFundingTx(
        wallet: wallet,
        unsignedTxB58: txB58,
        rpcUrl: svc.rpcUrl,
      );

      setState(() {
        _statusText = 'USDC sent!';
        _currentStep = 3;
        _isBusy = false;
      });
    } catch (e) {
      setState(() {
        _errorMessage = e.toString();
        _statusText = '';
        _isBusy = false;
      });
    }
  }

  // ── Step 3: Authorize ─────────────────────────────────────────────────
  Future<void> _onApprove() async {
    if (_isBusy) return;
    setState(() {
      _isBusy = true;
      _errorMessage = null;
      _statusText = 'Authorizing...';
    });

    try {
      await _sendAuthResponseWithExternalKey(_sessionKeyInfo!);
      setState(() => _statusText = 'Authorized');
      await Future.delayed(const Duration(milliseconds: 800));
      if (mounted) Navigator.of(context).pop('authorized');
    } catch (e) {
      setState(() {
        _errorMessage = e.toString();
        _statusText = '';
        _isBusy = false;
      });
    }
  }

  Future<void> _sendAuthResponseWithExternalKey(session.SessionKeyInfo info) async {
    final dailyLimitLamports = (_parseSol(_dailySpendingLimit) * 1000000000).round();
    final perTxLimitLamports = (_parseSol(_perTxLimit) * 1000000000).round();
    final dailyTxCount = int.tryParse(_dailyTxCountLimit) ?? 50;
    final durationSecs = (int.tryParse(_durationHours) ?? 24) * 3600;

    // Persist merchant policy
    try {
      final dir = await getApplicationSupportDirectory();
      if (_merchantDid.isNotEmpty) {
        await rust.saveMerchantPolicy(
          storagePath: dir.path,
          merchantDid: _merchantDid,
          dailySpendingLimit: BigInt.from(dailyLimitLamports),
          dailyTxCountLimit: dailyTxCount,
          perTxLimit: BigInt.from(perTxLimitLamports),
          durationSecs: durationSecs,
        );

        final prefs = await SharedPreferences.getInstance();
        final known = prefs.getStringList('known_merchant_dids') ?? [];
        if (!known.contains(_merchantDid)) {
          known.add(_merchantDid);
          await prefs.setStringList('known_merchant_dids', known);
        }
      }
    } catch (e) {
      debugPrint('Failed to save merchant policy: $e');
    }

    await rust.sendAuthResponse(
      storagePath: DidcommService().storagePath,
      paymentId: _paymentId,
      authorized: true,
      listAction: _listAction,
      mcpDid: DidcommService().pairedMcps.isNotEmpty
          ? DidcommService().pairedMcps.first.did
          : '',
      sessionKeyInfo: info,
      listLabel: _showLabelInput && _listLabel.isNotEmpty ? _listLabel : null,
      listMaxAmount: _showMaxAmountInput && _listMaxAmount.isNotEmpty
          ? int.tryParse(_listMaxAmount) != null
              ? BigInt.from(int.parse(_listMaxAmount))
              : null
          : null,
      dailyTxCountLimit: dailyTxCount,
      perTxLimit: BigInt.from(perTxLimitLamports),
      tokenMint: widget.request?.newSessionKeyTokenMint,
      paymentMethod: null,
    );

    DidcommService().clearPendingAuth();

    // Save payment record
    final svc = SessionKeyService();
    await svc.savePaymentRecord(
      paymentId: _paymentId,
      merchantDid: _merchantDid,
      amount: BigInt.from(_amount),
      tokenMint: widget.request?.tokenMint ?? info.sessionPda,
      description: _description,
      authorized: true,
      sessionKeyPubkey: info.ephemeralPubkey,
      txSignature: info.txSignature,
    );
  }

  void _onDecline() {
    Navigator.of(context).pop('declined');
  }

  // ── Build methods for each step ───────────────────────────────────────

  Widget _buildStepIndicator() {
    return Row(
      children: List.generate(4, (i) {
        final isActive = i == _currentStep;
        final isDone = i < _currentStep;
        return Expanded(
          child: Row(
            children: [
              Expanded(
                child: Container(
                  height: 3,
                  decoration: BoxDecoration(
                    color: isDone
                        ? _kSuccess
                        : isActive
                            ? _kAmber
                            : _kSurfaceMid,
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
              ),
              if (i < 3) const SizedBox(width: 4),
            ],
          ),
        );
      }),
    );
  }

  Widget _buildStepHeader() {
    return Column(
      children: [
        _buildStepIndicator(),
        const SizedBox(height: 16),
        Row(
          children: [
            Container(
              width: 36,
              height: 36,
              decoration: BoxDecoration(
                color: _kAmber.withValues(alpha: 0.15),
                borderRadius: BorderRadius.circular(10),
                border: Border.all(color: _kAmber.withValues(alpha: 0.3)),
              ),
              child: Icon(_stepIcons[_currentStep], size: 18, color: _kAmber),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Step ${_currentStep + 1} of 4',
                    style: GoogleFonts.inter(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: _kTextSecondary,
                      letterSpacing: 1.0,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    _stepTitles[_currentStep],
                    style: GoogleFonts.inter(
                      fontSize: 15,
                      fontWeight: FontWeight.w700,
                      color: _kTextPrimary,
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

  Widget _buildCreatePdaStep() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _buildStepHeader(),
        const SizedBox(height: 20),
        _MerchantCard(merchantDid: _merchantDid),
        const SizedBox(height: 16),
        _AmountDisplay(amount: _amount, tokenMint: widget.request?.tokenMint),
        const SizedBox(height: 8),
        _ReasonBlock(description: _description),
        const SizedBox(height: 16),
        _AuthorizationPolicyCard(
          dailySpendingLimit: _dailySpendingLimit,
          onDailySpendingLimitChanged: (v) => setState(() => _dailySpendingLimit = v),
          dailyTxCountLimit: _dailyTxCountLimit,
          onDailyTxCountLimitChanged: (v) => setState(() => _dailyTxCountLimit = v),
          perTxLimit: _perTxLimit,
          onPerTxLimitChanged: (v) => setState(() => _perTxLimit = v),
          durationHours: _durationHours,
          onDurationHoursChanged: (v) => setState(() => _durationHours = v),
        ),
        const SizedBox(height: 20),
        Row(
          children: [
            Expanded(
              child: Text(
                'Wallet: ${_walletLabel(_resolveWallet())}',
                style: GoogleFonts.inter(fontSize: 12, color: _kTextSecondary),
              ),
            ),
            TextButton(
              onPressed: _isBusy ? null : _switchWallet,
              child: Text(
                'Switch',
                style: GoogleFonts.inter(fontSize: 12, color: _kAmber),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        _ActionButton(
          label: 'Create PDA',
          icon: LucideIcons.keyRound,
          onTap: _isBusy ? null : _onCreatePda,
          isBusy: _isBusy,
          statusText: _statusText,
        ),
      ],
    );
  }

  Widget _buildFundSolStep() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _buildStepHeader(),
        const SizedBox(height: 20),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            color: _kSurfaceDark.withValues(alpha: 0.7),
            borderRadius: BorderRadius.circular(14),
            border: Border.all(color: _kAmber.withValues(alpha: 0.2)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(LucideIcons.coins, size: 14, color: _kAmber),
                  const SizedBox(width: 6),
                  Text(
                    'SEND SOL FOR GAS FEES',
                    style: GoogleFonts.inter(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: _kAmber,
                      letterSpacing: 1.0,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Text(
                'SOL is needed for transaction fees on the session key.',
                style: GoogleFonts.inter(fontSize: 11, color: _kTextSecondary),
              ),
              if (_ephemeralPubkey != null) ...[
                const SizedBox(height: 8),
                Text(
                  'To: ${_ephemeralPubkey!.substring(0, 8)}...${_ephemeralPubkey!.substring(_ephemeralPubkey!.length - 6)}',
                  style: GoogleFonts.jetBrainsMono(fontSize: 11, color: _kTextSecondary),
                ),
              ],
              const SizedBox(height: 14),
              _PolicyRow(
                label: 'Amount',
                value: _solFundingAmount,
                onChanged: (v) => setState(() => _solFundingAmount = v),
                suffix: 'SOL',
                keyboardType: const TextInputType.numberWithOptions(decimal: true),
              ),
            ],
          ),
        ),
        const SizedBox(height: 20),
        _ActionButton(
          label: 'Send SOL',
          icon: LucideIcons.send,
          onTap: _isBusy ? null : _onFundSol,
          isBusy: _isBusy,
          statusText: _statusText,
        ),
      ],
    );
  }

  Widget _buildFundUsdcStep() {
    final tokenMint = widget.request?.newSessionKeyTokenMint
        ?? '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _buildStepHeader(),
        const SizedBox(height: 20),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            color: _kSurfaceDark.withValues(alpha: 0.7),
            borderRadius: BorderRadius.circular(14),
            border: Border.all(color: _kAmber.withValues(alpha: 0.2)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(LucideIcons.banknote, size: 14, color: _kAmber),
                  const SizedBox(width: 6),
                  Text(
                    'SEND USDC TO SESSION KEY',
                    style: GoogleFonts.inter(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: _kAmber,
                      letterSpacing: 1.0,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Text(
                'USDC will be used for payments from this session key.',
                style: GoogleFonts.inter(fontSize: 11, color: _kTextSecondary),
              ),
              if (_sessionPda != null) ...[
                const SizedBox(height: 8),
                Text(
                  'To PDA: ${_sessionPda!.substring(0, 8)}...${_sessionPda!.substring(_sessionPda!.length - 6)}',
                  style: GoogleFonts.jetBrainsMono(fontSize: 11, color: _kTextSecondary),
                ),
              ],
              const SizedBox(height: 4),
              Text(
                'Mint: ${tokenMint.substring(0, 8)}...',
                style: GoogleFonts.jetBrainsMono(fontSize: 10, color: _kTextSecondary.withValues(alpha: 0.6)),
              ),
              const SizedBox(height: 14),
              _PolicyRow(
                label: 'Amount',
                value: _usdcFundingAmount,
                onChanged: (v) => setState(() => _usdcFundingAmount = v),
                suffix: 'USDC',
                keyboardType: const TextInputType.numberWithOptions(decimal: true),
              ),
            ],
          ),
        ),
        const SizedBox(height: 20),
        _ActionButton(
          label: 'Send USDC',
          icon: LucideIcons.send,
          onTap: _isBusy ? null : _onFundUsdc,
          isBusy: _isBusy,
          statusText: _statusText,
        ),
      ],
    );
  }

  Widget _buildAuthorizeStep() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _buildStepHeader(),
        const SizedBox(height: 20),
        _MerchantCard(merchantDid: _merchantDid),
        const SizedBox(height: 16),
        _AmountDisplay(amount: _amount, tokenMint: widget.request?.tokenMint),
        const SizedBox(height: 8),
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
        if (_statusText.isNotEmpty) ...[
          const SizedBox(height: 12),
          _ResultBanner(result: _statusText),
        ],
        const SizedBox(height: 20),
        _ApproveButton(
          onTap: _isBusy ? null : _onApprove,
          isAuthorizing: _isBusy,
        ),
        const SizedBox(height: 10),
        _DeclineButton(onTap: _onDecline),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: _kBackground,
      body: Stack(
        children: [
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
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 24),
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    const SizedBox(height: 16),
                    // Header with close button
                    Row(
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
                              'Session Key Setup',
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
                    ),
                    const SizedBox(height: 20),
                    // Error message
                    if (_errorMessage != null) ...[
                      _ResultBanner(result: 'Error: $_errorMessage'),
                      const SizedBox(height: 12),
                    ],
                    // Current step content
                    switch (_currentStep) {
                      0 => _buildCreatePdaStep(),
                      1 => _buildFundSolStep(),
                      2 => _buildFundUsdcStep(),
                      _ => _buildAuthorizeStep(),
                    },
                    const SizedBox(height: 32),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Action Button (used in wizard steps)
// ---------------------------------------------------------------------------
class _ActionButton extends StatelessWidget {
  final String label;
  final IconData icon;
  final VoidCallback? onTap;
  final bool isBusy;
  final String statusText;

  const _ActionButton({
    required this.label,
    required this.icon,
    required this.onTap,
    required this.isBusy,
    required this.statusText,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        if (statusText.isNotEmpty && isBusy) ...[
          Padding(
            padding: const EdgeInsets.only(bottom: 10),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: _kAmber.withValues(alpha: 0.8),
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  statusText,
                  style: GoogleFonts.inter(fontSize: 12, color: _kTextSecondary),
                ),
              ],
            ),
          ),
        ],
        GestureDetector(
          onTap: onTap,
          child: Container(
            width: double.infinity,
            height: 52,
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(26),
              gradient: LinearGradient(
                colors: isBusy
                    ? [const Color(0xFF666680), const Color(0xFF666680)]
                    : [const Color(0xFFFFB300), const Color(0xFFFFC107)],
                begin: Alignment.centerLeft,
                end: Alignment.centerRight,
              ),
              boxShadow: [
                BoxShadow(
                  color: _kAmber.withValues(alpha: isBusy ? 0.0 : 0.3),
                  blurRadius: 16,
                  spreadRadius: 0,
                ),
              ],
            ),
            child: Center(
              child: isBusy
                  ? const SizedBox(
                      width: 20,
                      height: 20,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: _kBackground,
                      ),
                    )
                  : Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(icon, size: 18, color: _kBackground),
                        const SizedBox(width: 8),
                        Text(
                          label,
                          style: GoogleFonts.inter(
                            fontSize: 15,
                            fontWeight: FontWeight.w700,
                            color: _kBackground,
                            letterSpacing: 0.5,
                          ),
                        ),
                      ],
                    ),
            ),
          ),
        ),
      ],
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
class _MerchantCard extends StatefulWidget {
  final String? merchantDid;
  const _MerchantCard({this.merchantDid});

  @override
  State<_MerchantCard> createState() => _MerchantCardState();
}

class _MerchantCardState extends State<_MerchantCard> {
  rust.MerchantProfile? _profile;
  bool _loading = true;

  String get _merchantDid => widget.merchantDid ?? '';

  String get _displayDid {
    if (_merchantDid.isNotEmpty) {
      if (_merchantDid.length > 24) {
        return '${_merchantDid.substring(0, 16)}...${_merchantDid.substring(_merchantDid.length - 6)}';
      }
      return _merchantDid;
    }
    return 'did:solana:7kPx...mN3q';
  }

  @override
  void initState() {
    super.initState();
    _resolveProfile();
  }

  Future<void> _resolveProfile() async {
    if (_merchantDid.isEmpty) {
      if (mounted) setState(() => _loading = false);
      return;
    }

    try {
      final dir = await getApplicationSupportDirectory();
      // Try cache first
      var profile = await rust.loadCachedMerchantProfile(
        storagePath: dir.path,
        merchantDid: _merchantDid,
      );
      // If not cached, fetch from registry
      if (profile == null) {
        final prefs = await SharedPreferences.getInstance();
        final registryUrl = prefs.getString('did_registry_url') ?? '';
        if (registryUrl.isNotEmpty) {
          profile = await rust.fetchMerchantProfile(
            registryUrl: registryUrl,
            merchantDid: _merchantDid,
          );
          await rust.saveCachedMerchantProfile(
            storagePath: dir.path,
            profile: profile,
          );
        }
      }
      if (mounted) {
        setState(() {
          _profile = profile;
          _loading = false;
        });
      }
    } catch (e) {
      debugPrint('Failed to resolve merchant profile: $e');
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final displayName = _profile?.name ?? 'Merchant';
    final isVerified = _profile?.verified ?? false;
    final category = _profile?.category;

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
                    Flexible(
                      child: Text(
                        displayName,
                        style: GoogleFonts.inter(
                          fontSize: 16,
                          fontWeight: FontWeight.w600,
                          color: _kTextPrimary,
                        ),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    const SizedBox(width: 8),
                    if (isVerified)
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
                      )
                    else if (!_loading)
                      Container(
                        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                        decoration: BoxDecoration(
                          color: _kAmber.withValues(alpha: 0.12),
                          borderRadius: BorderRadius.circular(6),
                          border: Border.all(color: _kAmber.withValues(alpha: 0.3)),
                        ),
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            const Icon(LucideIcons.alertTriangle, size: 11, color: _kAmber),
                            const SizedBox(width: 3),
                            Text(
                              'Unverified',
                              style: GoogleFonts.inter(
                                fontSize: 9,
                                fontWeight: FontWeight.w600,
                                color: _kAmber,
                              ),
                            ),
                          ],
                        ),
                      ),
                  ],
                ),
                if (category != null && category.isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(
                    category,
                    style: GoogleFonts.inter(
                      fontSize: 11,
                      color: _kTextSecondary.withValues(alpha: 0.8),
                    ),
                  ),
                ],
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
  final String? tokenMint;
  const _AmountDisplay({required this.amount, this.tokenMint});

  /// USDC mint addresses on Solana
  static const _usdcMints = [
    'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // mainnet
    '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU', // devnet
  ];

  bool get _isUsdc => tokenMint != null && _usdcMints.contains(tokenMint);

  String get _displayAmount {
    if (_isUsdc) {
      final usdc = amount / 1000000.0;
      if (usdc >= 1.0) {
        return usdc.toStringAsFixed(usdc.truncateToDouble() == usdc ? 0 : 2);
      }
      return usdc.toStringAsFixed(4).replaceAll(RegExp(r'0+$'), '').replaceAll(RegExp(r'\.$'), '');
    }
    final sol = amount / 1000000000.0;
    if (sol >= 1.0) {
      return sol.toStringAsFixed(sol.truncateToDouble() == sol ? 0 : 2);
    }
    return sol.toStringAsFixed(4).replaceAll(RegExp(r'0+$'), '').replaceAll(RegExp(r'\.$'), '');
  }

  String get _currencyLabel => _isUsdc ? 'USDC' : 'SOL';

  @override
  Widget build(BuildContext context) {
    // Debug: log tokenMint and _isUsdc result
    AppLogService().info('AmountDisplay', 'tokenMint="$tokenMint", isUsdc=$_isUsdc, amount=$amount');
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
              _displayAmount,
              style: GoogleFonts.inter(
                fontSize: 52,
                fontWeight: FontWeight.w800,
                color: _kTextPrimary,
                height: 1.0,
              ),
            ),
            const SizedBox(width: 6),
            Text(
              _currencyLabel,
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
              softWrap: true,
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Authorization Policy Card (editable parameters)
// ---------------------------------------------------------------------------
class _AuthorizationPolicyCard extends StatelessWidget {
  final String dailySpendingLimit;
  final ValueChanged<String> onDailySpendingLimitChanged;
  final String dailyTxCountLimit;
  final ValueChanged<String> onDailyTxCountLimitChanged;
  final String perTxLimit;
  final ValueChanged<String> onPerTxLimitChanged;
  final String durationHours;
  final ValueChanged<String> onDurationHoursChanged;

  const _AuthorizationPolicyCard({
    required this.dailySpendingLimit,
    required this.onDailySpendingLimitChanged,
    required this.dailyTxCountLimit,
    required this.onDailyTxCountLimitChanged,
    required this.perTxLimit,
    required this.onPerTxLimitChanged,
    required this.durationHours,
    required this.onDurationHoursChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.7),
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: _kAmber.withValues(alpha: 0.2)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(LucideIcons.shieldCheck, size: 14, color: _kAmber),
              const SizedBox(width: 6),
              Text(
                'AUTHORIZATION POLICY',
                style: GoogleFonts.inter(
                  fontSize: 10,
                  fontWeight: FontWeight.w600,
                  color: _kAmber,
                  letterSpacing: 1.0,
                ),
              ),
            ],
          ),
          const SizedBox(height: 14),
          _PolicyRow(
            label: 'Daily Limit',
            value: dailySpendingLimit,
            onChanged: onDailySpendingLimitChanged,
            suffix: 'SOL',
            keyboardType: const TextInputType.numberWithOptions(decimal: true),
          ),
          const SizedBox(height: 10),
          _PolicyRow(
            label: 'Daily Tx Count',
            value: dailyTxCountLimit,
            onChanged: onDailyTxCountLimitChanged,
            suffix: 'tx',
            keyboardType: TextInputType.number,
          ),
          const SizedBox(height: 10),
          _PolicyRow(
            label: 'Per-Tx Limit',
            value: perTxLimit,
            onChanged: onPerTxLimitChanged,
            suffix: 'SOL',
            keyboardType: const TextInputType.numberWithOptions(decimal: true),
          ),
          const SizedBox(height: 10),
          _PolicyRow(
            label: 'Duration',
            value: durationHours,
            onChanged: onDurationHoursChanged,
            suffix: 'hours',
            keyboardType: TextInputType.number,
          ),
        ],
      ),
    );
  }
}

class _PolicyRow extends StatelessWidget {
  final String label;
  final String value;
  final ValueChanged<String> onChanged;
  final String suffix;
  final TextInputType? keyboardType;

  const _PolicyRow({
    required this.label,
    required this.value,
    required this.onChanged,
    required this.suffix,
    this.keyboardType,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(
          width: 90,
          child: Text(
            label,
            style: GoogleFonts.inter(
              fontSize: 12,
              color: _kTextSecondary,
            ),
          ),
        ),
        Expanded(
          child: Container(
            height: 36,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            decoration: BoxDecoration(
              color: _kSurfaceMid.withValues(alpha: 0.6),
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: _kGlassBorder),
            ),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    onChanged: onChanged,
                    controller: TextEditingController(text: value)
                      ..selection = TextSelection.collapsed(offset: value.length),
                    style: GoogleFonts.jetBrainsMono(
                      fontSize: 13,
                      color: _kTextPrimary,
                    ),
                    decoration: InputDecoration(
                      border: InputBorder.none,
                      isDense: true,
                      contentPadding: const EdgeInsets.only(top: 8),
                    ),
                    keyboardType: keyboardType,
                  ),
                ),
                const SizedBox(width: 6),
                Text(
                  suffix,
                  style: GoogleFonts.inter(
                    fontSize: 11,
                    color: _kTextSecondary,
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Approve Button (Green gradient)
// ---------------------------------------------------------------------------
class _ApproveButton extends StatelessWidget {
  final VoidCallback? onTap;
  final bool isAuthorizing;

  const _ApproveButton({
    required this.onTap,
    this.isAuthorizing = false,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: double.infinity,
        height: 52,
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(26),
          gradient: const LinearGradient(
            colors: [Color(0xFF00C853), Color(0xFF00E676)],
            begin: Alignment.centerLeft,
            end: Alignment.centerRight,
          ),
          boxShadow: [
            BoxShadow(
              color: _kSuccess.withValues(alpha: 0.3),
              blurRadius: 16,
              spreadRadius: 0,
            ),
          ],
        ),
        child: Center(
          child: isAuthorizing
              ? const SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: _kBackground,
                  ),
                )
              : Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(LucideIcons.shieldCheck, size: 18, color: _kBackground),
                    const SizedBox(width: 8),
                    Text(
                      'APPROVE',
                      style: GoogleFonts.inter(
                        fontSize: 15,
                        fontWeight: FontWeight.w700,
                        color: _kBackground,
                        letterSpacing: 1.0,
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
