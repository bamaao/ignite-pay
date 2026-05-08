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
import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:ignite_pay_app/src/rust/api/simple.dart' as bridge;
import 'package:ignite_pay_app/services/channel_service.dart';
import 'package:ignite_pay_app/services/didcomm_service.dart';
import 'package:ignite_pay_app/services/direct_payment_service.dart';
import 'package:ignite_pay_app/theme.dart';

/// QR Payment Confirmation Screen.
///
/// Shown after scanning a merchant's payment QR code.
/// Displays payment details, lets user select payment method, and confirms.
class QrPaymentScreen extends StatefulWidget {
  final PaymentQrData paymentData;
  final String storagePath;
  final Future<ChannelPaymentResult> Function({
    required String storagePath,
    required String channelId,
    required String hubEndpoint,
    required int amount,
    required String recipientPubkey,
  }) onConfirmPayment;

  const QrPaymentScreen({
    super.key,
    required this.paymentData,
    required this.storagePath,
    required this.onConfirmPayment,
  });

  @override
  State<QrPaymentScreen> createState() => _QrPaymentScreenState();
}

class _QrPaymentScreenState extends State<QrPaymentScreen> {
  bool _isProcessing = false;
  bool _isSuccess = false;
  String? _errorMessage;
  ChannelPaymentResult? _result;
  String? _paymentProof;

  /// Available payment methods for the user to choose from.
  static const _paymentMethods = [
    _PaymentMethodOption('session_key', 'Session Key', '链上直接转账', Icons.key),
    _PaymentMethodOption('magicblock', 'MagicBlock', '链下 Voucher 支付', Icons.bolt),
    _PaymentMethodOption('local_wallet', 'Local Wallet', '本地钱包直接转账', Icons.account_balance_wallet),
    _PaymentMethodOption('sponsored', 'Sponsored', '代付模式 Gas 由 Relayer 支付', Icons.receipt_long),
  ];

  /// Token mint address mapping for common SPL tokens.
  static const _tokenMintMap = {
    'USDC': 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v',
    'USDT': 'Es9vMFrzaCERmGJgLVCesjWxagAanRn4CJr6YcxEPfBe',
    'SOL': '',
  };

  String _selectedMethod = 'session_key';
  String _selectedToken = 'SOL';
  StreamSubscription<QrPaymentResult>? _qrResultSub;
  final _directPaySvc = DirectPaymentService();

  @override
  void initState() {
    super.initState();
    // If merchant provides MB pubkey, default to MagicBlock
    if (widget.paymentData.merchantMbPubkey.isNotEmpty) {
      _selectedMethod = 'magicblock';
    }
    // Set default token from merchant's accepted tokens
    if (widget.paymentData.acceptTokens.isNotEmpty) {
      _selectedToken = widget.paymentData.acceptTokens.first;
    }
    // Listen for qr-payment-response from MCP
    _qrResultSub = DidcommService().qrPaymentResults.listen(_onQrPaymentResult);
  }

  @override
  void dispose() {
    _qrResultSub?.cancel();
    _directPaySvc.reset();
    super.dispose();
  }

  void _onQrPaymentResult(QrPaymentResult result) {
    if (result.orderId != widget.paymentData.orderId) return;
    if (!mounted) return;

    setState(() {
      _isProcessing = false;
      if (result.success) {
        _isSuccess = true;
        _paymentProof = result.paymentProof;
      } else {
        _errorMessage = result.error ?? 'Payment failed';
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            children: [
              // Header
              Row(
                children: [
                  BackButtonGlass(
                    onTap: _isProcessing ? null : () => Navigator.of(context).pop(),
                  ),
                  const Spacer(),
                  Text(
                    '确认支付',
                    style: GoogleFonts.inter(
                      fontSize: 18,
                      fontWeight: FontWeight.w700,
                      color: kTextPrimary,
                    ),
                  ),
                  const Spacer(),
                  const SizedBox(width: 36),
                ],
              ),
              const SizedBox(height: 32),

              // Merchant info card
              _buildMerchantCard(),
              const SizedBox(height: 24),

              // Amount
              Text(
                ChannelService.formatAmount(widget.paymentData.amount),
                style: GoogleFonts.jetBrainsMono(
                  fontSize: 36,
                  fontWeight: FontWeight.w700,
                  color: kNeonCyan,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                widget.paymentData.description,
                style: GoogleFonts.inter(
                  fontSize: 14,
                  color: kTextSecondary,
                ),
              ),
              const SizedBox(height: 24),

              // Token selector (only show when multiple tokens available)
              if (widget.paymentData.acceptTokens.length > 1) ...[
                _buildTokenSelector(),
                const SizedBox(height: 16),
              ],

              // Payment method selector
              _buildPaymentMethodSelector(),

              // Wallet connection UI (only for local_wallet method)
              if (_selectedMethod == 'local_wallet') ...[
                const SizedBox(height: 16),
                _buildWalletConnectSection(),
              ],

              const Spacer(),

              // Error message
              if (_errorMessage != null) ...[
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: kDanger.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: kDanger.withValues(alpha: 0.3)),
                  ),
                  child: Text(
                    _errorMessage!,
                    style: GoogleFonts.inter(fontSize: 12, color: kDanger),
                  ),
                ),
                const SizedBox(height: 16),
              ],

              // Success message
              if (_isSuccess) ...[
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: kSuccess.withValues(alpha: 0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: kSuccess.withValues(alpha: 0.3)),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '支付成功',
                        style: GoogleFonts.inter(fontSize: 14, fontWeight: FontWeight.w700, color: kSuccess),
                      ),
                      if (_paymentProof != null && _paymentProof!.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(top: 4),
                          child: Text(
                            _paymentProof!,
                            style: GoogleFonts.jetBrainsMono(fontSize: 11, color: kSuccess),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                    ],
                  ),
                ),
                const SizedBox(height: 16),
              ],

              // Action buttons
              if (!_isSuccess) ...[
                SizedBox(
                  width: double.infinity,
                  height: 48,
                  child: ElevatedButton(
                    onPressed: _isProcessing ? null : _onConfirm,
                    style: ElevatedButton.styleFrom(
                      backgroundColor: kNeonCyan,
                      foregroundColor: kBackground,
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    child: _isProcessing
                        ? const SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(
                              strokeWidth: 2,
                              color: kBackground,
                            ),
                          )
                        : Text(
                            '确认支付',
                            style: GoogleFonts.inter(
                              fontSize: 15,
                              fontWeight: FontWeight.w700,
                            ),
                          ),
                  ),
                ),
                const SizedBox(height: 8),
                SizedBox(
                  width: double.infinity,
                  height: 44,
                  child: TextButton(
                    onPressed: _isProcessing
                        ? null
                        : () => Navigator.of(context).pop(),
                    style: TextButton.styleFrom(
                      foregroundColor: kTextSecondary,
                    ),
                    child: Text(
                      '取消',
                      style: GoogleFonts.inter(fontSize: 14),
                    ),
                  ),
                ),
              ] else ...[
                SizedBox(
                  width: double.infinity,
                  height: 48,
                  child: ElevatedButton(
                    onPressed: () => Navigator.of(context).pop(_result ?? true),
                    style: ElevatedButton.styleFrom(
                      backgroundColor: kSuccess,
                      foregroundColor: kBackground,
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    child: Text(
                      '完成',
                      style: GoogleFonts.inter(
                        fontSize: 15,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildMerchantCard() {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: glassCardDecoration(),
      child: Row(
        children: [
          Container(
            width: 40,
            height: 40,
            decoration: BoxDecoration(
              color: kPurple.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: kPurple.withValues(alpha: 0.2)),
            ),
            child: const Icon(Icons.store, size: 20, color: kPurple),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '商户',
                  style: GoogleFonts.inter(
                    fontSize: 14,
                    fontWeight: FontWeight.w600,
                    color: kTextPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  _shortenDid(widget.paymentData.merchantDid),
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11,
                    color: kTextSecondary,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildTokenSelector() {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '币种选择',
            style: GoogleFonts.inter(
              fontSize: 10,
              fontWeight: FontWeight.w700,
              color: kTextTertiary,
              letterSpacing: 1.5,
            ),
          ),
          const SizedBox(height: 10),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: widget.paymentData.acceptTokens.map((token) {
                final isSelected = _selectedToken == token;
                return Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: GestureDetector(
                    onTap: _isProcessing ? null : () => setState(() => _selectedToken = token),
                    child: Container(
                      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                      decoration: BoxDecoration(
                        color: isSelected ? kNeonCyan.withValues(alpha: 0.1) : kSurfaceMid.withValues(alpha: 0.6),
                        borderRadius: BorderRadius.circular(20),
                        border: Border.all(
                          color: isSelected ? kNeonCyan : kGlassBorder,
                          width: isSelected ? 1.5 : 1,
                        ),
                      ),
                      child: Text(
                        token,
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: isSelected ? kNeonCyan : kTextPrimary,
                        ),
                      ),
                    ),
                  ),
                );
              }).toList(),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildPaymentMethodSelector() {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '支付方式',
            style: GoogleFonts.inter(
              fontSize: 10,
              fontWeight: FontWeight.w700,
              color: kTextTertiary,
              letterSpacing: 1.5,
            ),
          ),
          const SizedBox(height: 10),
          ..._paymentMethods.map((method) => _buildMethodOption(method)),
        ],
      ),
    );
  }

  Widget _buildMethodOption(_PaymentMethodOption method) {
    final isSelected = _selectedMethod == method.id;
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: InkWell(
        onTap: _isProcessing ? null : () => setState(() => _selectedMethod = method.id),
        borderRadius: BorderRadius.circular(10),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          decoration: BoxDecoration(
            color: isSelected ? kNeonCyan.withValues(alpha: 0.1) : Colors.transparent,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(
              color: isSelected ? kNeonCyan : kGlassBorder,
              width: isSelected ? 1.5 : 1,
            ),
          ),
          child: Row(
            children: [
              Icon(method.icon, size: 18, color: isSelected ? kNeonCyan : kTextSecondary),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      method.label,
                      style: GoogleFonts.inter(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: isSelected ? kNeonCyan : kTextPrimary,
                      ),
                    ),
                    Text(
                      method.subtitle,
                      style: GoogleFonts.inter(fontSize: 11, color: kTextSecondary),
                    ),
                  ],
                ),
              ),
              if (isSelected)
                const Icon(Icons.check_circle, size: 18, color: kNeonCyan),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildWalletConnectSection() {
    return AnimatedBuilder(
      animation: _directPaySvc,
      builder: (context, _) {
        final svc = _directPaySvc;

        if (svc.isConnecting) {
          // Waiting for wallet connect callback
          return Container(
            padding: const EdgeInsets.all(16),
            decoration: glassDecoration(),
            child: Row(
              children: [
                const SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(strokeWidth: 2, color: kNeonCyan),
                ),
                const SizedBox(width: 12),
                Text(
                  '等待钱包响应...',
                  style: GoogleFonts.inter(fontSize: 13, color: kTextSecondary),
                ),
              ],
            ),
          );
        }

        if (svc.walletPubkey != null) {
          // Connected
          final shortAddr = _shortenAddress(svc.walletPubkey!);
          return Container(
            padding: const EdgeInsets.all(16),
            decoration: glassDecoration(accentBorder: kSuccess.withValues(alpha: 0.3)),
            child: Row(
              children: [
                const Icon(Icons.check_circle, size: 18, color: kSuccess),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        svc.walletType == 'phantom' ? 'Phantom' : 'Solflare',
                        style: GoogleFonts.inter(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: kSuccess,
                        ),
                      ),
                      Text(
                        shortAddr,
                        style: GoogleFonts.jetBrainsMono(fontSize: 11, color: kTextSecondary),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          );
        }

        // Not connected: show wallet selection buttons
        return Container(
          padding: const EdgeInsets.all(16),
          decoration: glassDecoration(),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                '选择钱包连接',
                style: GoogleFonts.inter(
                  fontSize: 10,
                  fontWeight: FontWeight.w700,
                  color: kTextTertiary,
                  letterSpacing: 1.5,
                ),
              ),
              const SizedBox(height: 10),
              Row(
                children: [
                  Expanded(
                    child: _WalletButton(
                      label: 'Phantom',
                      icon: Icons.account_balance_wallet,
                      onTap: () => _connectWallet('phantom'),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: _WalletButton(
                      label: 'Solflare',
                      icon: Icons.account_balance_wallet_outlined,
                      onTap: () => _connectWallet('solflare'),
                    ),
                  ),
                ],
              ),
            ],
          ),
        );
      },
    );
  }

  Future<void> _connectWallet(String walletType) async {
    try {
      await _directPaySvc.connectWallet(walletType);
    } catch (e) {
      if (mounted) {
        setState(() => _errorMessage = 'Wallet connect failed: $e');
      }
    }
  }

  String _shortenAddress(String address) {
    if (address.length <= 12) return address;
    return '${address.substring(0, 4)}...${address.substring(address.length - 4)}';
  }

  String _shortenDid(String did) {
    if (did.length <= 24) return did;
    return '${did.substring(0, 16)}...${did.substring(did.length - 8)}';
  }

  Future<void> _onConfirm() async {
    setState(() {
      _isProcessing = true;
      _errorMessage = null;
    });

    try {
      if (_selectedMethod == 'local_wallet') {
        await _onConfirmDirectPayment();
        return;
      }

      if (_selectedMethod == 'sponsored') {
        await _onConfirmSponsoredPayment();
        return;
      }

      if (_selectedMethod == 'magicblock' && widget.paymentData.merchantMbPubkey.isNotEmpty) {
        // MB direct path: sign voucher locally and send to merchant
        await _onConfirmMbPayment();
        return;
      }

      // MCP-mediated path: send qr-payment-request, wait for qr-payment-response
      await bridge.sendQrPaymentRequest(
        storagePath: widget.storagePath,
        merchantDid: widget.paymentData.merchantDid,
        amount: BigInt.from(widget.paymentData.amount),
        description: widget.paymentData.description,
        orderId: widget.paymentData.orderId,
        paymentMethod: _selectedMethod,
        token: _selectedToken,
        merchantMediatorUrl: widget.paymentData.merchantMediatorUrl,
      );

      // Response will arrive via DidcommService.qrPaymentResults stream
      // _onQrPaymentResult will update the UI when it arrives
    } catch (e) {
      if (mounted) {
        setState(() {
          _isProcessing = false;
          _errorMessage = e.toString();
        });
      }
    }
  }

  Future<void> _onConfirmDirectPayment() async {
    const rpcUrl = String.fromEnvironment('SOLANA_RPC_URL', defaultValue: 'https://api.devnet.solana.com');
    final tokenMint = _tokenMintMap[_selectedToken] ?? '';

    final result = await _directPaySvc.executePayment(
      rpcUrl: rpcUrl,
      merchantDid: widget.paymentData.merchantDid,
      amountLamports: widget.paymentData.amount,
      token: _selectedToken,
      tokenMint: tokenMint,
      merchantWallet: widget.paymentData.merchantWallet.isNotEmpty
          ? widget.paymentData.merchantWallet
          : null,
    );

    if (!mounted) return;

    setState(() {
      _isProcessing = false;
      if (result.success) {
        _isSuccess = true;
        _paymentProof = result.signature;
      } else {
        _errorMessage = result.error ?? 'Direct payment failed';
      }
    });
  }

  Future<void> _onConfirmSponsoredPayment() async {
    const rpcUrl = String.fromEnvironment('SOLANA_RPC_URL', defaultValue: 'https://api.devnet.solana.com');
    const relayerUrl = String.fromEnvironment('RELAYER_URL', defaultValue: 'http://localhost:3030');
    final tokenMint = _tokenMintMap[_selectedToken] ?? '';

    final result = await _directPaySvc.executeSponsoredPayment(
      rpcUrl: rpcUrl,
      merchantDid: widget.paymentData.merchantDid,
      amountLamports: widget.paymentData.amount,
      relayerUrl: relayerUrl,
      token: _selectedToken,
      tokenMint: tokenMint,
      merchantWallet: widget.paymentData.merchantWallet.isNotEmpty
          ? widget.paymentData.merchantWallet
          : null,
    );

    if (!mounted) return;

    setState(() {
      _isProcessing = false;
      if (result.success) {
        _isSuccess = true;
        _paymentProof = result.signature;
      } else {
        _errorMessage = result.error ?? 'Sponsored payment failed';
      }
    });
  }

  Future<void> _onConfirmMbPayment() async {
    const mbProgramId = String.fromEnvironment('MB_PROGRAM_ID', defaultValue: '');

    try {
      await bridge.mbGetBuyerPubkey(storagePath: widget.storagePath);

      final voucher = await bridge.mbSignVoucher(
        storagePath: widget.storagePath,
        programId: mbProgramId,
        merchantMbPubkey: widget.paymentData.merchantMbPubkey,
        seq: BigInt.from(1),
        amount: BigInt.from(widget.paymentData.amount),
      );

      await bridge.mbSendVoucher(
        storagePath: widget.storagePath,
        merchantDid: widget.paymentData.merchantDid,
        orderId: widget.paymentData.orderId,
        channelId: voucher.channelId,
        seq: voucher.seq,
        amount: voucher.amount,
        buyerPubkey: voucher.buyerPubkey,
        buyerSig: voucher.buyerSig,
      );

      if (mounted) {
        setState(() {
          _isProcessing = false;
          _isSuccess = true;
          _result = ChannelPaymentResult(
            channelId: voucher.channelId,
            sequence: voucher.seq.toInt(),
            leafIndex: 0,
            newRoot: '',
          );
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _isProcessing = false;
          _errorMessage = e.toString();
        });
      }
    }
  }
}

class _PaymentMethodOption {
  final String id;
  final String label;
  final String subtitle;
  final IconData icon;
  const _PaymentMethodOption(this.id, this.label, this.subtitle, this.icon);
}

/// Wallet selection button widget.
class _WalletButton extends StatelessWidget {
  final String label;
  final IconData icon;
  final VoidCallback onTap;

  const _WalletButton({
    required this.label,
    required this.icon,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 12, horizontal: 14),
        decoration: BoxDecoration(
          color: kSurfaceMid.withValues(alpha: 0.6),
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: kGlassBorder),
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(icon, size: 16, color: kNeonCyan),
            const SizedBox(width: 8),
            Text(
              label,
              style: GoogleFonts.inter(
                fontSize: 13,
                fontWeight: FontWeight.w600,
                color: kTextPrimary,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
