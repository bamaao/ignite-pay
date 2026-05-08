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
import 'package:ignite_pay_app/theme.dart';
import 'package:ignite_pay_app/services/cctp_service.dart';
import 'package:ignite_pay_app/src/rust/api/cctp_transfer.dart';

/// Navigate to the CCTP transfer screen.
void openCctpTransfer(BuildContext context) {
  Navigator.of(context).push<void>(
    PageRouteBuilder(
      pageBuilder: (context, animation, secondaryAnimation) => const CctpTransferScreen(),
      transitionDuration: const Duration(milliseconds: 350),
      transitionsBuilder: (context, anim, secondaryAnimation, child) =>
          SlideTransition(
            position: Tween<Offset>(
              begin: const Offset(1, 0),
              end: Offset.zero,
            ).animate(CurvedAnimation(parent: anim, curve: Curves.easeOutCubic)),
            child: child,
          ),
    ),
  );
}

class CctpTransferScreen extends StatefulWidget {
  const CctpTransferScreen({super.key});

  @override
  State<CctpTransferScreen> createState() => _CctpTransferScreenState();
}

class _CctpTransferScreenState extends State<CctpTransferScreen> {
  final _svc = CctpService();
  final _amountController = TextEditingController();
  final _solanaAddrController = TextEditingController();

  int _selectedChainIndex = 1; // Default: Base (cheapest fees)
  String? _mintRecipientHex;
  String? _approveCalldata;
  String? _burnCalldata;
  String? _burnTxHash;
  String? _copiedField;

  static const _irisApiUrl = 'https://iris-api.circle.com';

  @override
  void initState() {
    super.initState();
    _svc.addListener(_onServiceUpdate);
  }

  @override
  void dispose() {
    _svc.removeListener(_onServiceUpdate);
    _svc.reset();
    _amountController.dispose();
    _solanaAddrController.dispose();
    super.dispose();
  }

  void _onServiceUpdate() {
    if (mounted) setState(() {});
  }

  // ── Step 1: Fetch fees + derive ATA ──────────────────────────────────

  Future<void> _startFlow() async {
    final amountText = _amountController.text.trim();
    if (amountText.isEmpty) return;

    final amount = double.tryParse(amountText);
    if (amount == null || amount <= 0) return;

    final solanaAddr = _solanaAddrController.text.trim();
    if (solanaAddr.isEmpty) return;

    final chain = CctpService.supportedChains[_selectedChainIndex];

    try {
      // Derive mint recipient (Solana USDC ATA)
      _mintRecipientHex = await _svc.deriveSolanaUsdcAta(solanaAddr);

      // Fetch fees
      await _svc.fetchFees(
        irisApiUrl: _irisApiUrl,
        srcDomain: chain.domainId,
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Error: $e'), backgroundColor: kDanger),
        );
      }
    }
  }

  // ── Step 2: Build + send approve tx ──────────────────────────────────

  Future<void> _doApprove() async {
    final chain = CctpService.supportedChains[_selectedChainIndex];
    final amount = (double.parse(_amountController.text) * 1e6).round();

    try {
      _approveCalldata = await _svc.buildApproveCalldata(
        chain: chain,
        amount: amount,
      );
      await _svc.openApproveTx(
        chain: chain,
        approveCalldata: _approveCalldata!,
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Approve error: $e'), backgroundColor: kDanger),
        );
      }
    }
  }

  // ── Step 3: Build + send burn tx ─────────────────────────────────────

  Future<void> _doBurn() async {
    final chain = CctpService.supportedChains[_selectedChainIndex];
    final amount = (double.parse(_amountController.text) * 1e6).round();

    try {
      _burnCalldata = await _svc.buildBurnCalldata(
        chain: chain,
        amount: amount,
        mintRecipientHex: _mintRecipientHex!,
      );
      await _svc.openBurnTx(
        chain: chain,
        burnCalldata: _burnCalldata!,
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Burn error: $e'), backgroundColor: kDanger),
        );
      }
    }
  }

  // ── Step 4: Poll status ──────────────────────────────────────────────

  Future<void> _startPolling(String txHash) async {
    final chain = CctpService.supportedChains[_selectedChainIndex];
    _burnTxHash = txHash;
    await _svc.pollStatus(
      irisApiUrl: _irisApiUrl,
      srcDomain: chain.domainId,
      burnTxHash: txHash,
    );
  }

  // ── Copy to clipboard ────────────────────────────────────────────────

  Future<void> _copyField(String label, String value) async {
    await Clipboard.setData(ClipboardData(text: value));
    setState(() => _copiedField = label);
    Future.delayed(const Duration(seconds: 2), () {
      if (mounted) setState(() => _copiedField = null);
    });
  }

  // ── Build ────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBackground,
      body: SafeArea(
        child: CustomScrollView(
          slivers: [
            // Header
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
                child: PageHeader(
                  title: 'Cross-chain Deposit',
                  subtitle: 'EVM → Solana via CCTP',
                ),
              ),
            ),

            // Content
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(20, 20, 20, 40),
                child: _buildContent(),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildContent() {
    final state = _svc.state;

    if (state == CctpState.done) return _buildDone();
    if (state == CctpState.error) return _buildError();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Step indicator
        _buildStepIndicator(state),
        const SizedBox(height: 24),

        // Form (always visible, disabled during processing)
        _buildForm(enabled: state == CctpState.idle || state == CctpState.fetchingFees),
        const SizedBox(height: 20),

        // Fee display
        if (_svc.feeQuote != null) ...[
          _buildFeeDisplay(),
          const SizedBox(height: 20),
        ],

        // Action buttons
        _buildActions(state),

        // Manual calldata fallback
        if (_approveCalldata != null && state == CctpState.approving) ...[
          const SizedBox(height: 16),
          _buildCalldataFallback('Approve Calldata', _approveCalldata!),
        ],
        if (_burnCalldata != null && state == CctpState.burning) ...[
          const SizedBox(height: 16),
          _buildCalldataFallback('Burn Calldata', _burnCalldata!),
        ],

        // Polling input
        if (state == CctpState.burning || state == CctpState.polling) ...[
          const SizedBox(height: 16),
          _buildPollingInput(state),
        ],
      ],
    );
  }

  // ── Step indicator ───────────────────────────────────────────────────

  Widget _buildStepIndicator(CctpState state) {
    final steps = [
      ('Approve', CctpState.approving),
      ('Burn', CctpState.burning),
      ('Mint', CctpState.polling),
    ];

    int activeStep = -1;
    if (state == CctpState.approving) activeStep = 0;
    if (state == CctpState.burning) activeStep = 1;
    if (state == CctpState.polling) activeStep = 2;
    if (state == CctpState.done) activeStep = 3;

    return Row(
      children: steps.asMap().entries.map((entry) {
        final idx = entry.key;
        final (label, _) = entry.value;
        final isActive = idx == activeStep;
        final isDone = idx < activeStep;

        return Expanded(
          child: Row(
            children: [
              Expanded(
                child: Column(
                  children: [
                    Container(
                      width: 28,
                      height: 28,
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: isDone
                            ? kSuccess
                            : isActive
                                ? kNeonCyan
                                : kSurfaceElevated,
                        border: isActive
                            ? Border.all(color: kNeonCyan, width: 2)
                            : null,
                      ),
                      child: Center(
                        child: isDone
                            ? const Icon(Icons.check, size: 16, color: kBackground)
                            : Text(
                                '${idx + 1}',
                                style: GoogleFonts.inter(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w600,
                                  color: isActive ? kBackground : kTextSecondary,
                                ),
                              ),
                      ),
                    ),
                    const SizedBox(height: 6),
                    Text(
                      label,
                      style: GoogleFonts.inter(
                        fontSize: 11,
                        color: isActive ? kTextPrimary : kTextSecondary,
                        fontWeight: isActive ? FontWeight.w600 : FontWeight.w400,
                      ),
                    ),
                  ],
                ),
              ),
              if (idx < 2)
                Expanded(
                  child: Container(
                    height: 2,
                    color: isDone ? kSuccess : kSurfaceElevated,
                  ),
                ),
            ],
          ),
        );
      }).toList(),
    );
  }

  // ── Form ─────────────────────────────────────────────────────────────

  Widget _buildForm({required bool enabled}) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const SectionLabel(text: 'SOURCE CHAIN'),
        const SizedBox(height: 8),
        _buildChainDropdown(enabled),
        const SizedBox(height: 16),
        const SectionLabel(text: 'AMOUNT (USDC)'),
        const SizedBox(height: 8),
        _buildTextField(
          controller: _amountController,
          hint: '0.00',
          enabled: enabled,
          keyboardType: const TextInputType.numberWithOptions(decimal: true),
        ),
        const SizedBox(height: 16),
        const SectionLabel(text: 'SOLANA RECIPIENT'),
        const SizedBox(height: 8),
        _buildTextField(
          controller: _solanaAddrController,
          hint: 'Solana wallet address (base58)',
          enabled: enabled,
        ),
      ],
    );
  }

  Widget _buildChainDropdown(bool enabled) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 4),
      decoration: glassDecoration(),
      child: DropdownButtonHideUnderline(
        child: DropdownButton<int>(
          value: _selectedChainIndex,
          isExpanded: true,
          dropdownColor: kSurfaceDark,
          style: GoogleFonts.inter(fontSize: 14, color: kTextPrimary),
          icon: const Icon(Icons.arrow_drop_down, color: kTextSecondary),
          items: CctpService.supportedChains.asMap().entries.map((entry) {
            return DropdownMenuItem<int>(
              value: entry.key,
              child: Text(entry.value.name),
            );
          }).toList(),
          onChanged: enabled
              ? (val) {
                  if (val != null) setState(() => _selectedChainIndex = val);
                }
              : null,
        ),
      ),
    );
  }

  Widget _buildTextField({
    required TextEditingController controller,
    required String hint,
    required bool enabled,
    TextInputType? keyboardType,
  }) {
    return TextField(
      controller: controller,
      enabled: enabled,
      keyboardType: keyboardType,
      style: GoogleFonts.inter(fontSize: 14, color: kTextPrimary),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: GoogleFonts.inter(fontSize: 14, color: kTextTertiary),
        filled: true,
        fillColor: kSurfaceDark,
        contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
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
      ),
    );
  }

  // ── Fee display ──────────────────────────────────────────────────────

  Widget _buildFeeDisplay() {
    final quote = _svc.feeQuote!;
    return Container(
      padding: const EdgeInsets.all(14),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Fee Estimate', style: cardTitle()),
          const SizedBox(height: 10),
          _feeRow('Transfer amount', '${_amountController.text} USDC'),
          _feeRow('Forwarding fee (med)', '${quote.forwardFeeMed} USDC'),
          _feeRow('Est. received', _estimatedReceived(quote)),
        ],
      ),
    );
  }

  Widget _feeRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Text(label, style: cardSubtitle()),
          Text(value, style: monoValue(12)),
        ],
      ),
    );
  }

  String _estimatedReceived(CctpFeeQuote quote) {
    final amount = double.tryParse(_amountController.text) ?? 0;
    final fee = double.tryParse(quote.forwardFeeMed) ?? 0;
    final received = amount - fee;
    return received > 0 ? '${received.toStringAsFixed(6)} USDC' : '—';
  }

  // ── Action buttons ───────────────────────────────────────────────────

  Widget _buildActions(CctpState state) {
    switch (state) {
      case CctpState.idle:
        return SizedBox(
          width: double.infinity,
          child: ElevatedButton(
            onPressed: _startFlow,
            style: ElevatedButton.styleFrom(
              backgroundColor: kNeonCyan,
              foregroundColor: kBackground,
              padding: const EdgeInsets.symmetric(vertical: 14),
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
            ),
            child: Text(
              'Get Fee Quote',
              style: GoogleFonts.inter(fontSize: 15, fontWeight: FontWeight.w600),
            ),
          ),
        );

      case CctpState.fetchingFees:
        return const Center(child: CircularProgressIndicator(color: kNeonCyan));

      case CctpState.approving:
        return SizedBox(
          width: double.infinity,
          child: ElevatedButton(
            onPressed: _doApprove,
            style: ElevatedButton.styleFrom(
              backgroundColor: kNeonCyan,
              foregroundColor: kBackground,
              padding: const EdgeInsets.symmetric(vertical: 14),
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
            ),
            child: Text(
              'Open MetaMask — Approve USDC',
              style: GoogleFonts.inter(fontSize: 15, fontWeight: FontWeight.w600),
            ),
          ),
        );

      case CctpState.burning:
        return SizedBox(
          width: double.infinity,
          child: ElevatedButton(
            onPressed: _doBurn,
            style: ElevatedButton.styleFrom(
              backgroundColor: kNeonCyan,
              foregroundColor: kBackground,
              padding: const EdgeInsets.symmetric(vertical: 14),
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
            ),
            child: Text(
              'Open MetaMask — Send Burn Tx',
              style: GoogleFonts.inter(fontSize: 15, fontWeight: FontWeight.w600),
            ),
          ),
        );

      case CctpState.polling:
        return const Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            SizedBox(
              width: 20,
              height: 20,
              child: CircularProgressIndicator(color: kNeonCyan, strokeWidth: 2),
            ),
            SizedBox(width: 12),
            Text(
              'Waiting for Circle attestation...',
              style: TextStyle(color: kPending, fontSize: 14),
            ),
          ],
        );

      default:
        return const SizedBox.shrink();
    }
  }

  // ── Calldata fallback ────────────────────────────────────────────────

  Widget _buildCalldataFallback(String label, String calldata) {
    final isCopied = _copiedField == label;
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(label, style: cardSubtitle()),
              GestureDetector(
                onTap: () => _copyField(label, calldata),
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                  decoration: BoxDecoration(
                    color: isCopied ? kSuccess.withValues(alpha: 0.15) : kSurfaceElevated,
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: Text(
                    isCopied ? 'Copied!' : 'Copy',
                    style: GoogleFonts.inter(
                      fontSize: 12,
                      color: isCopied ? kSuccess : kTextSecondary,
                    ),
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            '${calldata.substring(0, calldata.length > 66 ? 66 : calldata.length)}...',
            style: monoValue(10),
          ),
          const SizedBox(height: 4),
          Text(
            'If MetaMask deep link fails, manually send this calldata to the contract in your wallet.',
            style: GoogleFonts.inter(fontSize: 10, color: kTextTertiary),
          ),
        ],
      ),
    );
  }

  // ── Polling input ────────────────────────────────────────────────────

  Widget _buildPollingInput(CctpState state) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: glassDecoration(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Manual Status Check', style: cardSubtitle()),
          const SizedBox(height: 8),
          if (_burnTxHash != null)
            Text('Tracking: ${_burnTxHash!.substring(0, 20)}...', style: monoValue(10)),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: TextField(
                  style: GoogleFonts.inter(fontSize: 12, color: kTextPrimary),
                  decoration: InputDecoration(
                    hintText: 'Paste burn tx hash if not auto-detected',
                    hintStyle: GoogleFonts.inter(fontSize: 11, color: kTextTertiary),
                    filled: true,
                    fillColor: kSurfaceDark,
                    isDense: true,
                    contentPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(8),
                      borderSide: const BorderSide(color: kBorder),
                    ),
                  ),
                  onSubmitted: (hash) {
                    if (hash.isNotEmpty) _startPolling(hash);
                  },
                ),
              ),
              const SizedBox(width: 8),
              ElevatedButton(
                onPressed: () {
                  // Get hash from the field
                },
                style: ElevatedButton.styleFrom(
                  backgroundColor: kSurfaceElevated,
                  foregroundColor: kTextPrimary,
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  minimumSize: Size.zero,
                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
                ),
                child: Text('Poll', style: GoogleFonts.inter(fontSize: 12)),
              ),
            ],
          ),
        ],
      ),
    );
  }

  // ── Done ─────────────────────────────────────────────────────────────

  Widget _buildDone() {
    final fwdHash = _svc.forwardTxHash ?? '';
    return Column(
      children: [
        const SizedBox(height: 40),
        Container(
          width: 64,
          height: 64,
          decoration: const BoxDecoration(
            shape: BoxShape.circle,
            color: kSuccess,
          ),
          child: const Icon(Icons.check, size: 32, color: kBackground),
        ),
        const SizedBox(height: 20),
        Text(
          'Deposit Complete',
          style: GoogleFonts.inter(
            fontSize: 22,
            fontWeight: FontWeight.w700,
            color: kTextPrimary,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          '${_amountController.text} USDC has been deposited to your Solana wallet.',
          style: GoogleFonts.inter(fontSize: 14, color: kTextSecondary),
          textAlign: TextAlign.center,
        ),
        if (fwdHash.isNotEmpty) ...[
          const SizedBox(height: 24),
          Container(
            padding: const EdgeInsets.all(14),
            decoration: glassDecoration(),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Solana Transaction', style: cardTitle()),
                const SizedBox(height: 8),
                GestureDetector(
                  onTap: () => _copyField('fwdHash', fwdHash),
                  child: Row(
                    children: [
                      Expanded(
                        child: Text(
                          fwdHash.length > 40 ? '${fwdHash.substring(0, 40)}...' : fwdHash,
                          style: monoValue(12),
                        ),
                      ),
                      Icon(
                        _copiedField == 'fwdHash' ? Icons.check : Icons.copy,
                        size: 16,
                        color: kTextSecondary,
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 10),
                GestureDetector(
                  onTap: () {
                    // Open Solscan link
                  },
                  child: Text(
                    'View on Solscan',
                    style: GoogleFonts.inter(
                      fontSize: 13,
                      color: kNeonCyan,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
        const SizedBox(height: 24),
        SizedBox(
          width: double.infinity,
          child: ElevatedButton(
            onPressed: () {
              _svc.reset();
              Navigator.of(context).pop();
            },
            style: ElevatedButton.styleFrom(
              backgroundColor: kSurfaceElevated,
              foregroundColor: kTextPrimary,
              padding: const EdgeInsets.symmetric(vertical: 14),
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
            ),
            child: Text(
              'Done',
              style: GoogleFonts.inter(fontSize: 15, fontWeight: FontWeight.w600),
            ),
          ),
        ),
      ],
    );
  }

  // ── Error ────────────────────────────────────────────────────────────

  Widget _buildError() {
    return Column(
      children: [
        const SizedBox(height: 40),
        Container(
          width: 64,
          height: 64,
          decoration: const BoxDecoration(
            shape: BoxShape.circle,
            color: kDanger,
          ),
          child: const Icon(Icons.close, size: 32, color: kBackground),
        ),
        const SizedBox(height: 20),
        Text(
          'Transfer Failed',
          style: GoogleFonts.inter(
            fontSize: 22,
            fontWeight: FontWeight.w700,
            color: kTextPrimary,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          _svc.errorMessage ?? 'Unknown error',
          style: GoogleFonts.inter(fontSize: 13, color: kDanger),
          textAlign: TextAlign.center,
          maxLines: 3,
          overflow: TextOverflow.ellipsis,
        ),
        const SizedBox(height: 24),
        Row(
          children: [
            Expanded(
              child: ElevatedButton(
                onPressed: () {
                  _svc.reset();
                  setState(() {
                    _approveCalldata = null;
                    _burnCalldata = null;
                    _burnTxHash = null;
                  });
                },
                style: ElevatedButton.styleFrom(
                  backgroundColor: kNeonCyan,
                  foregroundColor: kBackground,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                ),
                child: Text(
                  'Retry',
                  style: GoogleFonts.inter(fontSize: 15, fontWeight: FontWeight.w600),
                ),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: ElevatedButton(
                onPressed: () => Navigator.of(context).pop(),
                style: ElevatedButton.styleFrom(
                  backgroundColor: kSurfaceElevated,
                  foregroundColor: kTextPrimary,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                ),
                child: Text(
                  'Close',
                  style: GoogleFonts.inter(fontSize: 15, fontWeight: FontWeight.w600),
                ),
              ),
            ),
          ],
        ),
      ],
    );
  }
}
