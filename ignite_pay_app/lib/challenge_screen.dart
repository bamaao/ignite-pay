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
import 'package:ignite_pay_app/services/phantom_wallet_service.dart';
import 'package:ignite_pay_app/services/session_key_service.dart';
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

  // Authorization policy fields (editable by user)
  String _dailySpendingLimit = ''; // SOL string, default = amount*10
  String _dailyTxCountLimit = '50';
  String _perTxLimit = ''; // SOL string, default = amount
  String _durationHours = '24';

  // Funding fields — visible only when creating a new session key
  String _solFundingAmount = '0.01'; // SOL
  String _usdcFundingAmount = '1.0'; // USDC

  // Wallet service — defaults to Phantom, user can switch via selector
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
      }
    }
  }

  Future<void> _onAuthorize() async {
    final svc = SessionKeyService();

    // F2: If MCP provided a new session key, always go through Phantom flow
    final mcpSessionKey = widget.request?.newSessionKeyPubkey;
    AppLogService().info('Auth', 'onAuthorize: newSessionKeyPubkey=$mcpSessionKey, tokenMint=${widget.request?.tokenMint}, hasRequest=${widget.request != null}');
    if (mcpSessionKey != null && mcpSessionKey.isNotEmpty) {
      setState(() {
        _isAuthorizing = true;
        _authResult = 'Connecting to wallet...';
      });
      try {
        final req = widget.request!;
        await svc.initialize();

        final dir = await getApplicationSupportDirectory();

        // 1. Connect to wallet (Phantom deep link or WC2)
        final wallet = _walletService ?? PhantomWalletService();
        await wallet.loadSession();
        if (!wallet.isConnected) {
          final connected = await wallet.connect();
          if (!connected) throw 'Failed to connect to wallet';
        }

        // 2. Check if this session key is already registered on-chain
        setState(() => _authResult = 'Checking session key status...');
        final onChainInfo = await rust.getSessionAccountInfo(
          rpcUrl: svc.rpcUrl,
          ownerB58: wallet.walletPublicKey!,
          ephemeralB58: req.newSessionKeyPubkey!,
        );
        AppLogService().info('Auth', 'onChain check: exists=${onChainInfo.exists}, revoked=${onChainInfo.revoked}, expiresAt=${onChainInfo.expiresAt}');

        if (onChainInfo.exists && !onChainInfo.revoked) {
          // ── Already-registered path: skip registration, finalize locally ──
          final scopes = req.newSessionKeyScopes ?? ['sol:transfer', 'spl:transfer'];
          final info = await rust.finalizeExistingSessionKey(
            storagePath: dir.path,
            ownerPubkeyB58: wallet.walletPublicKey!,
            ephemeralPubkey: req.newSessionKeyPubkey!,
            onChainInfo: onChainInfo,
            scopes: scopes,
            realSecretKey: req.newSessionKeySecretKey,
          );

          // Check balances and top-up if needed
          final pdaAddress = info.sessionPda ?? info.ephemeralPubkey;
          final needsFund = await _needsFunding(svc.rpcUrl, pdaAddress);
          if (needsFund) {
            await _fundSessionKeyViaWallet(
              wallet,
              info.ephemeralPubkey,
              info.sessionPda ?? info.ephemeralPubkey,
              svc.rpcUrl,
            );
          }

          // Send auth response
          await _sendAuthResponseWithExternalKey(info);
          setState(() => _authResult = 'Authorized with existing session key');
          await Future.delayed(const Duration(milliseconds: 1200));
          if (mounted) Navigator.of(context).pop('authorized');
          return;
        }

        // ── Not-registered path: register on-chain via wallet ──

        // Determine target program from scopes
        final isSpl = (req.newSessionKeyScopes ?? []).any((s) => s.contains('spl'));
        final targetProgram = isSpl
            ? 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'
            : '11111111111111111111111111111111';

        // 3. Build register tx with wallet owner, ephemeral pubkey from MCP
        setState(() => _authResult = 'Building register transaction...');
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

        // 4. Wallet signTransaction (signs but does not broadcast)
        setState(() => _authResult = 'Open wallet to sign register tx...');
        final signedRegisterTx = await wallet.signTransaction(unsignedRegister.unsignedTxB58);
        if (signedRegisterTx == null) throw 'Wallet rejected register transaction';

        // 5. Broadcast the fully signed register tx
        setState(() => _authResult = 'Broadcasting register transaction...');
        final registerSig = await session.broadcastSignedTx(
          rpcUrl: svc.rpcUrl,
          signedTxB58: signedRegisterTx,
        );

        // 6. Finalize: move pending key to permanent storage
        final info = await session.finalizePhantomSessionKey(
          storagePath: dir.path,
          ephemeralPubkey: unsignedRegister.ephemeralPubkey,
          txSignature: registerSig,
          sessionPda: unsignedRegister.sessionPda,
          realSecretKey: req.newSessionKeySecretKey,
        );

        // 7. Fund the new session key via wallet (user-customizable amounts)
        await _fundSessionKeyViaWallet(
          wallet,
          info.ephemeralPubkey,
          info.sessionPda ?? unsignedRegister.sessionPda,
          svc.rpcUrl,
        );

        // 8. Send auth response with the registered session key info
        await _sendAuthResponseWithExternalKey(info);
        setState(() => _authResult = 'Authorized with wallet-funded session key');
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

  /// Fund a session key via wallet using the user's custom SOL/USDC amounts.
  /// Sends SOL to the session PDA and USDC to the PDA's ATA.
  /// Also sends a small amount of SOL to the ephemeral key for gas fees.
  Future<void> _fundSessionKeyViaWallet(
    WalletService wallet,
    String ephemeralPubkey,
    String sessionPda,
    String rpcUrl,
  ) async {
    // 1. Send SOL to session PDA
    final solAmount = _parseSol(_solFundingAmount);
    if (solAmount > 0) {
      setState(() => _authResult = 'Open wallet to send SOL to PDA...');
      final solLamports = (solAmount * 1000000000).round();
      final txB58 = await session.buildUnsignedTransferTx(
        rpcUrl: rpcUrl,
        walletPubkeyB58: wallet.walletPublicKey!,
        merchantDid: sessionPda,
        amountLamports: BigInt.from(solLamports),
      );
      final sig = await wallet.signAndSendTransaction(txB58);
      if (sig == null) throw 'Wallet rejected SOL transfer';
    }

    // 2. Send gas SOL to ephemeral key (0.01 SOL)
    setState(() => _authResult = 'Open wallet to send gas SOL...');
    final gasTxB58 = await session.buildUnsignedTransferTx(
      rpcUrl: rpcUrl,
      walletPubkeyB58: wallet.walletPublicKey!,
      merchantDid: ephemeralPubkey,
      amountLamports: BigInt.from(10000000), // 0.01 SOL
    );
    final gasSig = await wallet.signAndSendTransaction(gasTxB58);
    if (gasSig == null) throw 'Wallet rejected gas SOL transfer';

    // 3. Send USDC to PDA's ATA
    final usdcAmount = double.tryParse(_usdcFundingAmount) ?? 0.0;
    final tokenMint = widget.request?.newSessionKeyTokenMint
        ?? '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU'; // devnet USDC
    if (usdcAmount > 0) {
      setState(() => _authResult = 'Open wallet to send USDC to PDA ATA...');
      final usdcRaw = (usdcAmount * 1000000).round(); // USDC has 6 decimals
      final txB58 = await session.buildUnsignedSplTransferTx(
        rpcUrl: rpcUrl,
        walletPubkeyB58: wallet.walletPublicKey!,
        merchantWalletB58: sessionPda,
        amount: BigInt.from(usdcRaw),
        tokenMintB58: tokenMint,
      );
      final sig = await wallet.signAndSendTransaction(txB58);
      if (sig == null) throw 'Wallet rejected USDC transfer';
    }
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

  /// Check whether a session key needs SOL or USDC funding.
  /// Returns true if either balance is below the user-specified funding amounts.
  Future<bool> _needsFunding(String rpcUrl, String ephemeralPubkey) async {
    final solBalance = await rust.getSolBalance(
      rpcUrl: rpcUrl,
      pubkeyB58: ephemeralPubkey,
    );
    final solThreshold = BigInt.from((_parseSol(_solFundingAmount) * 1000000000).round());
    if (solBalance < solThreshold) return true;

    final tokenMint = widget.request?.newSessionKeyTokenMint
        ?? '4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU'; // devnet USDC
    final usdcBalance = await rust.getTokenBalance(
      rpcUrl: rpcUrl,
      ownerPubkeyB58: ephemeralPubkey,
      tokenMintB58: tokenMint,
    );
    final usdcThreshold = BigInt.from(((double.tryParse(_usdcFundingAmount) ?? 0.0) * 1000000).round());
    if (usdcBalance < usdcThreshold) return true;

    return false;
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
                    // Show funding section only when creating a new session key
                    if (_isNewSessionKey && !_checkingExistingKey) ...[
                      _FundingCard(
                        solFundingAmount: _solFundingAmount,
                        onSolFundingAmountChanged: (v) => setState(() => _solFundingAmount = v),
                        usdcFundingAmount: _usdcFundingAmount,
                        onUsdcFundingAmountChanged: (v) => setState(() => _usdcFundingAmount = v),
                      ),
                      const SizedBox(height: 16),
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
        final registryUrl = prefs.getString('hub_registry_url') ?? '';
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
// Funding Card (SOL + USDC amounts for new session key)
// ---------------------------------------------------------------------------
class _FundingCard extends StatelessWidget {
  final String solFundingAmount;
  final ValueChanged<String> onSolFundingAmountChanged;
  final String usdcFundingAmount;
  final ValueChanged<String> onUsdcFundingAmountChanged;

  const _FundingCard({
    required this.solFundingAmount,
    required this.onSolFundingAmountChanged,
    required this.usdcFundingAmount,
    required this.onUsdcFundingAmountChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: _kSurfaceDark.withValues(alpha: 0.7),
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: const Color(0xFFAB9FF2).withValues(alpha: 0.3)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(LucideIcons.wallet, size: 14, color: Color(0xFFAB9FF2)),
              const SizedBox(width: 6),
              Text(
                'SESSION KEY FUNDING',
                style: GoogleFonts.inter(
                  fontSize: 10,
                  fontWeight: FontWeight.w600,
                  color: const Color(0xFFAB9FF2),
                  letterSpacing: 1.0,
                ),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text(
            'Fund via wallet (first-time setup)',
            style: GoogleFonts.inter(fontSize: 11, color: _kTextSecondary),
          ),
          const SizedBox(height: 14),
          _PolicyRow(
            label: 'SOL Amount',
            value: solFundingAmount,
            onChanged: onSolFundingAmountChanged,
            suffix: 'SOL',
            keyboardType: const TextInputType.numberWithOptions(decimal: true),
          ),
          const SizedBox(height: 10),
          _PolicyRow(
            label: 'USDC Amount',
            value: usdcFundingAmount,
            onChanged: onUsdcFundingAmountChanged,
            suffix: 'USDC',
            keyboardType: const TextInputType.numberWithOptions(decimal: true),
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
